//! Stream resolvers, a 1:1 port of the memoized JS resolvers:
//! `validateStream` (HEAD/GET probe of a direct HLS URL) and `getStream`
//! (yt-dlp `-g -f best` extraction for video IDs / channel handles).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method};
use url::Url;

use crate::state::AppState;

/// Realistic desktop user agents, standing in for the `user-agents` package.
const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:127.0) Gecko/20100101 Firefox/127.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:126.0) Gecko/20100101 Firefox/126.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:127.0) Gecko/20100101 Firefox/127.0",
    "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:127.0) Gecko/20100101 Firefox/127.0",
    "Mozilla/5.0 (X11; Linux x86_64; rv:126.0) Gecko/20100101 Firefox/126.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36 OPR/109.0.0.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36 OPR/109.0.0.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Vivaldi/6.8",
];

fn random_user_agent() -> &'static str {
    use rand::Rng;
    let mut rng = rand::rng();
    USER_AGENTS[rng.random_range(0..USER_AGENTS.len())]
}

/// JS `new URL()` is more lenient than `url::Url` about raw spaces in the
/// query; sanitize them so the behaviour matches.
fn parse_url_lenient(raw: &str) -> Result<Url, url::ParseError> {
    match Url::parse(raw) {
        Ok(u) => Ok(u),
        Err(_) => Url::parse(&raw.replace(' ', "%20")),
    }
}

/// Validate a direct HLS URL with a HEAD request (GET fallback on transport
/// errors). Custom headers may be embedded in the URL via the
/// `custom-header` query parameter ("name:value,name2:value2").
async fn validate_stream(raw: &str, state: &AppState) -> Option<String> {
    println!("🔍 Validating stream: {raw}");

    let parsed = match parse_url_lenient(raw) {
        Ok(u) => u,
        Err(e) => {
            println!("-> ❌ Failed to validate stream: {raw}\n{e}");
            return None;
        }
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static(random_user_agent()),
    );

    if let Some((_, custom)) = parsed
        .query_pairs()
        .find(|(k, _)| k.as_ref() == state.custom_header_key)
    {
        for pair in custom.split(',') {
            if let Some(idx) = pair.find(':') {
                let name = &pair[..idx];
                let value = &pair[idx + 1..];
                // Invalid header names/values make JS fetch throw; same here.
                if let (Ok(n), Ok(v)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
                    headers.insert(n, v);
                }
            }
        }
    }

    // Rebuild the URL without the custom-header parameter(s), like
    // `urlObj.searchParams.delete(...)` + `urlObj.toString()`. JS only
    // rewrites the query when it existed in the first place.
    let had_query = parsed.query().is_some();
    let retained: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| k.as_ref() != state.custom_header_key)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut clean = parsed;
    if had_query {
        if retained.is_empty() {
            // JS serializes a cleared query as absent — except when the
            // original URL literally ended in "?" (kept as-is).
            if clean.query() != Some("") {
                clean.set_query(None);
            }
        } else {
            let mut pairs = clean.query_pairs_mut();
            pairs.clear();
            for (k, v) in &retained {
                pairs.append_pair(k, v);
            }
        }
    }
    let clean_url = clean.to_string();

    // HEAD first; on transport errors retry once with GET (JS .catch()).
    let response = match state
        .client
        .request(Method::HEAD, &clean_url)
        .headers(headers.clone())
        .send()
        .await
    {
        Ok(r) => r,
        Err(head_err) => match state
            .client
            .request(Method::GET, &clean_url)
            .headers(headers)
            .send()
            .await
        {
            Ok(r) => r,
            Err(get_err) => {
                println!(
                    "-> ❌ Failed to validate stream: {raw}\nHEAD: {head_err}\nGET: {get_err}"
                );
                return None;
            }
        },
    };

    if response.status().as_u16() < 400 {
        Some(clean_url)
    } else {
        None
    }
}

/// Extract the best stream URL via yt-dlp (`-g -f best`), first stdout line.
async fn fetch_stream_url(url: &str, state: &AppState) -> Option<String> {
    println!("📺 Fetching HLS stream for: {url}");

    let mut command = tokio::process::Command::new("yt-dlp");
    command.args([
        "--quiet",
        "--no-warnings",
        "--no-progress",
        "--no-update",
        "-f",
        "best",
        "-g",
    ]);
    command.arg(url);
    if let Some(js_runtime) = &state.ytdlp_js_runtime {
        command.args(["--js-runtimes", js_runtime]);
    }
    // Point yt-dlp's web/API cache at CACHE_DIR so repeated extractions of
    // the same URL skip network round-trips (persisted via the volume).
    if let Some(cache_dir) = &state.ytdlp_cache_dir {
        command.args(["--cache-dir", cache_dir.to_str().unwrap_or_default()]);
    }

    let output = match state.ytdlp_timeout {
        Some(t) => match tokio::time::timeout(t, command.output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                println!("-> ❌ Failed to fetch HLS stream for: {url}\n{e}");
                return None;
            }
            Err(_) => {
                println!("-> ❌ Failed to fetch HLS stream for: {url}\nyt-dlp timed out");
                return None;
            }
        },
        None => match command.output().await {
            Ok(out) => out,
            Err(e) => {
                println!("-> ❌ Failed to fetch HLS stream for: {url}\n{e}");
                return None;
            }
        },
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        println!(
            "-> ❌ Failed to fetch HLS stream for: {url}\nyt-dlp exited with {}{}",
            output.status,
            if detail.is_empty() {
                String::new()
            } else {
                format!("\n{detail}")
            }
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout
        .trim()
        .split('\n')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if first.is_empty() {
        None
    } else {
        Some(first)
    }
}

/// A single query parameter (`v`, `c` or `x`).
#[derive(Clone, Debug)]
pub struct Param {
    pub kind: ParamKind,
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    Video,
    Channel,
    Direct,
}

/// Cache lifetime for a resolution result:
///   - successes: `MEMOIZATION_TTL`, capped by the URL's own `expire` token;
///   - failures: `CACHE_NEGATIVE_TTL` (short).
fn cache_ttl(state: &AppState, value: &Option<String>) -> Duration {
    match value {
        Some(url) => match url_expiry(url) {
            Some(expire_unix) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                state
                    .positive_ttl
                    .min(Duration::from_secs(expire_unix.saturating_sub(now)))
            }
            None => state.positive_ttl,
        },
        None => state.negative_ttl.unwrap_or_default(),
    }
}

/// Extract the `expire` token from a stream URL. YouTube/Google HLS manifests
/// embed it either as a query parameter (`?expire=1785868364`) or as a path
/// segment (`/expire/1785868364/`).
fn url_expiry(url: &str) -> Option<u64> {
    let parsed = Url::parse(url).ok()?;
    if let Some(v) = parsed
        .query_pairs()
        .find(|(k, _)| k == "expire")
        .and_then(|(_, v)| v.parse().ok())
    {
        return Some(v);
    }
    let segments: Vec<&str> = parsed.path_segments()?.collect();
    for (i, segment) in segments.iter().enumerate() {
        if *segment == "expire" {
            if let Some(v) = segments.get(i + 1).and_then(|s| s.parse().ok()) {
                return Some(v);
            }
        }
    }
    None
}

/// Resolve one parameter to an HLS URL, memoized with the same cache keys as
/// the JS port (`x:{raw}` for direct URLs, `v:{url}` for yt-dlp lookups).
pub async fn resolve_hls(state: &Arc<AppState>, param: &Param) -> Option<String> {
    let result = match param.kind {
        ParamKind::Direct => {
            let cached = state.cache.as_ref().map(|c| {
                let key = format!("x:{}", param.value);
                let state = Arc::clone(state);
                let param = param.clone();
                c.get_or_compute(key, move || async move {
                    let value = validate_stream(&param.value, &state).await;
                    let ttl = cache_ttl(&state, &value);
                    (value, ttl)
                })
            });
            match cached {
                Some(fut) => fut.await,
                None => validate_stream(&param.value, state).await,
            }
        }
        ParamKind::Video | ParamKind::Channel => {
            let url = match param.kind {
                ParamKind::Video => format!("https://www.youtube.com/watch?v={}", param.value),
                ParamKind::Channel => format!("https://www.youtube.com/{}/live", param.value),
                ParamKind::Direct => unreachable!(),
            };
            let cached = state.cache.as_ref().map(|c| {
                let key = format!("v:{url}");
                let state = Arc::clone(state);
                let url = url.clone();
                c.get_or_compute(key, move || async move {
                    let value = fetch_stream_url(&url, &state).await;
                    let ttl = cache_ttl(&state, &value);
                    (value, ttl)
                })
            });
            match cached {
                Some(fut) => fut.await,
                None => fetch_stream_url(&url, state).await,
            }
        }
    };
    if result.is_some() {
        println!("✅ HLS stream found!");
    }
    result
}

/// Current resident set size in bytes (Linux /proc; getrusage elsewhere).
pub fn current_rss_bytes() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(resident_pages) = statm.split_whitespace().nth(1) {
                if let Ok(pages) = resident_pages.parse::<u64>() {
                    // SAFETY: sysconf is a read-only libc query.
                    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                    if page_size > 0 {
                        return pages * page_size as u64;
                    }
                }
            }
        }
    }
    #[cfg(unix)]
    {
        // Fallback: maximum resident set size (KB on macOS, bytes on Linux).
        // SAFETY: getrusage writes into `usage`, which is zeroed first.
        let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0 {
            let kb = usage.ru_maxrss;
            if kb > 0 {
                return kb as u64 * 1024;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expire_from_query_param() {
        assert_eq!(
            url_expiry("https://x/y?expire=1785868364&a=1"),
            Some(1785868364)
        );
    }

    #[test]
    fn expire_from_path_segment() {
        assert_eq!(
            url_expiry("https://manifest.googlevideo.com/api/manifest/hls_playlist/expire/1785868364/ei/abc"),
            Some(1785868364)
        );
    }

    #[test]
    fn no_expire_token() {
        assert_eq!(url_expiry("https://example.com/live.m3u8"), None);
        assert_eq!(url_expiry("not a url"), None);
        assert_eq!(url_expiry("https://x/?expire=abc"), None);
    }

    #[tokio::test]
    async fn negative_ttl_when_disabled() {
        let cfg = crate::state::Config {
            hostname: "localhost".into(),
            port: 3000,
            api_key: None,
            custom_header_key: "custom-header".into(),
            memoization_ttl: Some(Duration::from_secs(1800)),
            cache_negative_ttl: None,
            cache_dir: None,
            cache_redis_url: None,
            cors_origin: "*".into(),
            ytdlp_js_runtime: None,
            ytdlp_timeout: None,
            request_timeout: None,
        };
        let state = crate::state::AppState::new(&cfg);
        // No negative caching configured: failures get zero lifetime.
        assert_eq!(cache_ttl(&state, &None), Duration::ZERO);
        // Positive entries use the full TTL when no expire token is present.
        assert_eq!(
            cache_ttl(&state, &Some("https://example.com/live.m3u8".into())),
            Duration::from_secs(1800)
        );
    }

    #[tokio::test]
    async fn positive_ttl_capped_by_expiry() {
        let cfg = crate::state::Config {
            hostname: "localhost".into(),
            port: 3000,
            api_key: None,
            custom_header_key: "custom-header".into(),
            memoization_ttl: Some(Duration::from_secs(1800)),
            cache_negative_ttl: Some(Duration::from_secs(120)),
            cache_dir: None,
            cache_redis_url: None,
            cors_origin: "*".into(),
            ytdlp_js_runtime: None,
            ytdlp_timeout: None,
            request_timeout: None,
        };
        let state = crate::state::AppState::new(&cfg);

        // Expire far in the future: TTL wins (capped at 30 min).
        let far = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 7200;
        assert_eq!(
            cache_ttl(&state, &Some(format!("https://x/expire/{far}/"))),
            Duration::from_secs(1800)
        );

        // Expire in 60s: URL expiry wins.
        let soon = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 60;
        let ttl = cache_ttl(&state, &Some(format!("https://x/expire/{soon}/")));
        assert!(ttl <= Duration::from_secs(60) && ttl > Duration::from_secs(50));

        // Already expired: nothing cached.
        let past = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(10);
        assert_eq!(
            cache_ttl(&state, &Some(format!("https://x/expire/{past}/"))),
            Duration::ZERO
        );
    }
}
