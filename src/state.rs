//! Configuration and shared application state.
//!
//! Environment variables mirror the original Bun implementation:
//!   HOSTNAME (fallback HOST, then "localhost"), PORT, API_KEY,
//!   CUSTOM_X_HEADER, MEMOIZATION_TTL, CORS_ORIGIN,
//!   plus optional enhancements: YTDLP_JS_RUNTIME, YTDLP_TIMEOUT,
//!   REQUEST_TIMEOUT, CACHE_NEGATIVE_TTL, CACHE_DIR, CACHE_REDIS_URL.

use std::path::PathBuf;
use std::time::Duration;

use reqwest::Client;

use crate::cache::{Backend, TtlCache};

/// Parsed environment configuration.
pub struct Config {
    pub hostname: String,
    pub port: u16,
    pub api_key: Option<String>,
    pub custom_header_key: String,
    /// `None` disables memoization entirely (TTL <= 0 or unparsable).
    pub memoization_ttl: Option<Duration>,
    /// How long failed lookups are cached (`CACHE_NEGATIVE_TTL`, default 2m).
    pub cache_negative_ttl: Option<Duration>,
    /// Shared cache root (`CACHE_DIR`): yt-dlp `--cache-dir` + persistent redb.
    /// `None` keeps everything in memory and leaves yt-dlp's default cache.
    pub cache_dir: Option<PathBuf>,
    /// Optional Redis URL (`CACHE_REDIS_URL`) — shared cache across instances.
    pub cache_redis_url: Option<String>,
    pub cors_origin: String,
    /// `--js-runtimes` value passed to yt-dlp, when present.
    pub ytdlp_js_runtime: Option<String>,
    /// Optional kill-switch for hanging yt-dlp invocations (0 = disabled).
    pub ytdlp_timeout: Option<Duration>,
    /// Optional timeout for stream validation requests (0 = disabled).
    pub request_timeout: Option<Duration>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Parse seconds as a float; invalid values behave like JS `Number()` => NaN => 0.
fn parse_seconds(value: Option<&str>) -> Option<Duration> {
    let n = value.and_then(|s| s.trim().parse::<f64>().ok())?;
    if n.is_nan() || n <= 0.0 {
        None
    } else {
        Some(Duration::from_secs_f64(n))
    }
}

/// Parse minutes as a float (JS `Number(x) * MINUTE` semantics).
fn parse_minutes(value: Option<&str>) -> Option<Duration> {
    let n = value.and_then(|s| s.trim().parse::<f64>().ok())?;
    if n.is_nan() || n <= 0.0 {
        None
    } else {
        Some(Duration::from_millis((n * 60_000.0) as u64))
    }
}

impl Config {
    pub fn from_env() -> Self {
        // HOSTNAME is what the original code reads; HOST is what the README
        // documents, so honour both.
        let hostname = env("HOSTNAME")
            .or_else(|| env("HOST"))
            .unwrap_or_else(|| "localhost".to_string());

        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.trim().parse::<u16>().ok())
            .unwrap_or(3000);

        let api_key = env("API_KEY");

        let custom_header_key =
            env("CUSTOM_X_HEADER").unwrap_or_else(|| "custom-header".to_string());

        // JS: Number(env ?? 30) * MINUTE; NaN / <= 0 => disabled.
        let memoization_ttl = parse_minutes(Some(
            std::env::var("MEMOIZATION_TTL").as_deref().unwrap_or("30"),
        ));

        // Failed lookups: cached briefly so dead streams don't hammer yt-dlp.
        let cache_negative_ttl = parse_minutes(Some(
            std::env::var("CACHE_NEGATIVE_TTL")
                .as_deref()
                .unwrap_or("2"),
        ));

        // Shared cache root: drives yt-dlp `--cache-dir` and the redb file.
        // Explicitly empty disables persistence; unset defaults to $HOME/cache
        // (the container image defaults this to /cache via ENV).
        let cache_dir = match std::env::var("CACHE_DIR") {
            Ok(v) if v.is_empty() => None,
            Ok(v) => Some(PathBuf::from(v)),
            Err(_) => std::env::var("HOME")
                .ok()
                .filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join("cache")),
        };

        let cache_redis_url = env("CACHE_REDIS_URL");

        let cors_origin = env("CORS_ORIGIN").unwrap_or_else(|| "*".to_string());

        // yt-dlp `--js-runtimes`. Explicitly set (even empty) => use as given;
        // unset => keep the original flag when the bun binary exists, otherwise
        // omit it so yt-dlp falls back to its bundled interpreter.
        let ytdlp_js_runtime = match std::env::var("YTDLP_JS_RUNTIME") {
            Ok(v) if !v.is_empty() => Some(v),
            Ok(_) => None,
            Err(_) => {
                const BUN: &str = "bun:/usr/local/bin/bun";
                std::path::Path::new("/usr/local/bin/bun")
                    .exists()
                    .then(|| BUN.to_string())
            }
        };

        let ytdlp_timeout = parse_seconds(env("YTDLP_TIMEOUT").as_deref());
        let request_timeout = parse_seconds(env("REQUEST_TIMEOUT").as_deref());

        Config {
            hostname,
            port,
            api_key,
            custom_header_key,
            memoization_ttl,
            cache_negative_ttl,
            cache_dir,
            cache_redis_url,
            cors_origin,
            ytdlp_js_runtime,
            ytdlp_timeout,
            request_timeout,
        }
    }
}

/// Shared state handed to handlers and middleware.
pub struct AppState {
    pub api_key: Option<String>,
    pub custom_header_key: String,
    pub cors_origin: String,
    pub cache: Option<TtlCache>,
    /// TTL for successful lookups (from MEMOIZATION_TTL).
    pub positive_ttl: Duration,
    /// TTL for failed lookups (CACHE_NEGATIVE_TTL); `None` = don't cache failures.
    pub negative_ttl: Option<Duration>,
    /// yt-dlp `--cache-dir` value (from CACHE_DIR).
    pub ytdlp_cache_dir: Option<PathBuf>,
    pub client: Client,
    pub ytdlp_js_runtime: Option<String>,
    pub ytdlp_timeout: Option<Duration>,
}

impl AppState {
    pub fn new(cfg: &Config) -> Self {
        let mut builder = Client::builder();
        if let Some(t) = cfg.request_timeout {
            builder = builder.timeout(t);
        }

        // Persistent backend: Redis when configured, else redb under CACHE_DIR.
        // Backend failures degrade to the in-memory cache, never to errors.
        let backend = if let Some(url) = &cfg.cache_redis_url {
            match redis::Client::open(url.clone()) {
                Ok(client) => Some(Backend::redis(client)),
                Err(e) => {
                    eprintln!("⚠️ CACHE_REDIS_URL invalid ({e}); falling back to local cache");
                    cfg.cache_dir.as_ref().and_then(|d| Backend::redb(d))
                }
            }
        } else {
            cfg.cache_dir.as_ref().and_then(|d| Backend::redb(d))
        };

        AppState {
            api_key: cfg.api_key.clone(),
            custom_header_key: cfg.custom_header_key.clone(),
            cors_origin: cfg.cors_origin.clone(),
            cache: cfg.memoization_ttl.map(|_| TtlCache::new(backend.clone())),
            positive_ttl: cfg.memoization_ttl.unwrap_or_default(),
            negative_ttl: cfg.cache_negative_ttl,
            ytdlp_cache_dir: cfg.cache_dir.clone(),
            client: builder.build().expect("failed to build HTTP client"),
            ytdlp_js_runtime: cfg.ytdlp_js_runtime.clone(),
            ytdlp_timeout: cfg.ytdlp_timeout,
        }
    }
}
