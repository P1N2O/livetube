//! Configuration and shared application state.
//!
//! Environment variables mirror the original Bun implementation:
//!   HOSTNAME (fallback HOST, then "localhost"), PORT, API_KEY,
//!   CUSTOM_X_HEADER, MEMOIZATION_TTL, CORS_ORIGIN,
//!   plus optional enhancements: YTDLP_JS_RUNTIME, YTDLP_TIMEOUT, REQUEST_TIMEOUT.

use std::time::Duration;

use reqwest::Client;

use crate::cache::TtlCache;

/// Parsed environment configuration.
pub struct Config {
    pub hostname: String,
    pub port: u16,
    pub api_key: Option<String>,
    pub custom_header_key: String,
    /// `None` disables memoization (TTL <= 0 or unparsable), like the JS port.
    pub memoization_ttl: Option<Duration>,
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
        let memoization_ttl = std::env::var("MEMOIZATION_TTL")
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|n| !n.is_nan() && *n > 0.0)
            .map(|minutes| Duration::from_millis((minutes * 60_000.0) as u64));

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
        AppState {
            api_key: cfg.api_key.clone(),
            custom_header_key: cfg.custom_header_key.clone(),
            cors_origin: cfg.cors_origin.clone(),
            cache: cfg.memoization_ttl.map(TtlCache::new),
            client: builder.build().expect("failed to build HTTP client"),
            ytdlp_js_runtime: cfg.ytdlp_js_runtime.clone(),
            ytdlp_timeout: cfg.ytdlp_timeout,
        }
    }
}
