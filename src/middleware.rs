//! Middleware replicating the original hono stack, in the same order:
//! logger -> compress -> cors -> auth (`every(logger, compress, cors,
//! some(...))` in the JS source).

use std::io::IsTerminal;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use flate2::read::{DeflateEncoder, GzEncoder};
use flate2::Compression;

use crate::fmt;
use crate::state::AppState;

// ─────────────────────────────────────────────
// Logger (hono/logger)
// ─────────────────────────────────────────────

pub async fn logger_mw(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    println!("\n<-- {method} {path}");
    let start = Instant::now();
    let res = next.run(req).await;
    let status = res.status();
    let elapsed = elapsed_ms(start);
    let status_str = if std::io::stdout().is_terminal() {
        color_status(status)
    } else {
        status.to_string()
    };
    println!("--> {method} {path} {status_str} {elapsed}");
    res
}

fn elapsed_ms(start: Instant) -> String {
    fmt::elapsed_str(start.elapsed().as_millis() as u64)
}

fn color_status(status: StatusCode) -> String {
    let code = status.as_u16();
    let color = match code / 100 {
        5 => "\x1b[31m", // red
        4 => "\x1b[33m", // yellow
        3 => "\x1b[36m", // cyan
        _ => "\x1b[32m", // green
    };
    format!("{color}{code}\x1b[0m")
}

// ─────────────────────────────────────────────
// Compression (hono/compress, threshold 1024)
// ─────────────────────────────────────────────

const THRESHOLD: u64 = 1024;

pub async fn compress_mw(req: Request, next: Next) -> Response {
    let is_head = req.method() == Method::HEAD;
    let accept_encoding = req
        .headers()
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let res = next.run(req).await;

    if is_head
        || res.headers().contains_key(header::CONTENT_ENCODING)
        || res.headers().contains_key(header::TRANSFER_ENCODING)
        || below_threshold(res.headers())
        || !should_compress(res.headers())
        || !should_transform(res.headers())
    {
        return res;
    }

    // hono: ENCODING_TYPES.find(e => accepted?.includes(e)) => gzip, then deflate.
    let encoding = if accept_encoding.contains("gzip") {
        "gzip"
    } else if accept_encoding.contains("deflate") {
        "deflate"
    } else {
        return res;
    };

    let (mut parts, body) = res.into_parts();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };
    if bytes.is_empty() {
        return Response::from_parts(parts, Body::empty());
    }

    let compressed = match encoding {
        "gzip" => gzip(&bytes),
        _ => deflate(&bytes),
    };

    parts.headers.remove(header::CONTENT_LENGTH);
    parts
        .headers
        .insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
    Response::from_parts(parts, Body::from(compressed))
}

fn below_threshold(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|len| len < THRESHOLD)
        .unwrap_or(false)
}

/// Port of hono's COMPRESSIBLE_CONTENT_TYPE_REGEX (practical subset covering
/// everything this server emits or would plausibly emit).
fn should_compress(headers: &HeaderMap) -> bool {
    let Some(content_type) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let ct = content_type.trim_start().to_ascii_lowercase();
    if ct.starts_with("text/") {
        return !ct.starts_with("text/event-stream");
    }
    if ct.starts_with("application/") {
        let rest = ct.strip_prefix("application/").unwrap_or(&ct);
        let base = rest.split(';').next().unwrap_or(rest);
        return matches!(
            base,
            "javascript"
                | "json"
                | "xml"
                | "xml-dtd"
                | "ecmascript"
                | "dart"
                | "postscript"
                | "rtf"
                | "tar"
                | "toml"
                | "vnd.dart"
                | "vnd.ms-fontobject"
                | "vnd.ms-opentype"
                | "wasm"
                | "x-httpd-php"
                | "x-javascript"
                | "x-ns-proxy-autoconfig"
                | "x-sh"
                | "x-tar"
                | "x-www-form-urlencoded"
        ) || base.ends_with("+json")
            || base.ends_with("+text")
            || base.ends_with("+xml")
            || base.ends_with("+yaml");
    }
    matches!(
        ct.split(';').next().unwrap_or(&ct),
        "font/otf"
            | "font/ttf"
            | "image/bmp"
            | "image/vnd.adobe.photoshop"
            | "image/vnd.microsoft.icon"
            | "image/vnd.ms-dds"
            | "image/x-icon"
            | "image/x-ms-bmp"
            | "message/rfc822"
            | "model/gltf-binary"
            | "x-shader/x-fragment"
            | "x-shader/x-vertex"
    )
}

fn should_transform(headers: &HeaderMap) -> bool {
    let Some(cache_control) = headers
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
    else {
        return true;
    };
    // /(?:^|,)\s*?no-transform\s*?(?:,|$)/i
    !cache_control
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case("no-transform"))
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut encoder = GzEncoder::new(bytes, Compression::default());
    let mut out = Vec::new();
    encoder.read_to_end(&mut out).expect("gzip encode failed");
    out
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut encoder = DeflateEncoder::new(bytes, Compression::default());
    let mut out = Vec::new();
    encoder
        .read_to_end(&mut out)
        .expect("deflate encode failed");
    out
}

// ─────────────────────────────────────────────
// CORS (hono/cors with origin: CORS_ORIGIN, methods: GET,POST,OPTIONS)
// ─────────────────────────────────────────────

const ALLOW_METHODS: &str = "GET,POST,OPTIONS";

pub async fn cors_mw(State(state): State<ArcState>, req: Request, next: Next) -> Response {
    let origin = &state.cors_origin;

    if req.method() == Method::OPTIONS {
        let mut headers = HeaderMap::new();
        if origin == "*" {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
        } else if let Some(req_origin) = req.headers().get(header::ORIGIN) {
            if req_origin.to_str().ok() == Some(origin.as_str()) {
                headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, req_origin.clone());
            }
        }
        if origin != "*" {
            headers.insert(header::VARY, HeaderValue::from_static("Origin"));
        }
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static(ALLOW_METHODS),
        );
        if let Some(request_headers) = req.headers().get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
            if let Ok(list) = request_headers.to_str() {
                // hono: split(/\s*,\s*/).join(",") — normalized list.
                let joined = list
                    .split(',')
                    .map(|h| h.trim())
                    .collect::<Vec<_>>()
                    .join(",");
                headers.insert(
                    header::ACCESS_CONTROL_ALLOW_HEADERS,
                    HeaderValue::from_str(&joined).unwrap_or(request_headers.clone()),
                );
                headers.append(
                    header::VARY,
                    HeaderValue::from_static("Access-Control-Request-Headers"),
                );
            }
        }
        let mut res = Response::new(Body::empty());
        *res.status_mut() = StatusCode::NO_CONTENT;
        res.headers_mut().extend(headers);
        return res;
    }

    // hono appends `Vary: Origin` on non-preflight responses when the origin
    // is not "*", and only reflects the origin when it matches.
    let req_origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|o| o.to_string());

    let mut res = next.run(req).await;

    if origin == "*" {
        res.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    } else {
        if req_origin.as_deref() == Some(origin.as_str()) {
            if let Ok(v) = HeaderValue::from_str(origin) {
                res.headers_mut()
                    .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
            }
        }
        res.headers_mut()
            .append(header::VARY, HeaderValue::from_static("Origin"));
    }
    res
}

// ─────────────────────────────────────────────
// Auth (hono: some(() => !API_KEY, UA bearerAuth, Authorization bearerAuth))
// ─────────────────────────────────────────────

/// Token charset from hono: `[A-Za-z0-9._~+/-]+=*` (full match, /i).
fn matches_token_charset(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut has_charset = false;
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'~' | b'+' | b'/' | b'-' => {
                has_charset = true;
            }
            b'=' => {}
            _ => return false,
        }
    }
    has_charset
}

/// Parse `Bearer <token>` (case-insensitive prefix, at least one space,
/// optional trailing spaces) — hono regex `^Bearer +([charset]+=*) *$`.
fn parse_bearer(auth: &str) -> Option<&str> {
    let rest = auth.strip_prefix("Bearer").or_else(|| {
        if auth.len() > 6 && auth[..6].eq_ignore_ascii_case("bearer") {
            Some(&auth[6..])
        } else {
            None
        }
    })?;
    let rest = rest.strip_prefix(' ')?;
    let token = rest.trim_end();
    matches_token_charset(token).then_some(token)
}

fn unauthorized(www_authenticate: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, www_authenticate)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from("Unauthorized"))
        .expect("401 response")
}

fn bad_request() -> Response {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(header::WWW_AUTHENTICATE, "Bearer error=\"invalid_request\"")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from("Bad Request"))
        .expect("400 response")
}

pub async fn auth_mw(State(state): State<ArcState>, req: Request, next: Next) -> Response {
    // `/health` is always open, even when an API key is configured.
    if req.uri().path() == "/health" {
        return next.run(req).await;
    }

    let Some(api_key) = &state.api_key else {
        return next.run(req).await;
    };

    // 1) User-Agent may carry the API key (hono UA bearerAuth, prefix "").
    if let Some(ua) = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
    {
        if matches_token_charset(ua) && ua.contains(api_key.as_str()) {
            return next.run(req).await;
        }
    }

    // 2) Standard `Authorization: Bearer <token>`.
    if let Some(auth) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        return match parse_bearer(auth) {
            Some(token) if token == api_key.as_str() => next.run(req).await,
            Some(_) => unauthorized("Bearer error=\"invalid_token\""),
            None => bad_request(),
        };
    }

    unauthorized("Bearer realm=\"\"")
}

// ─────────────────────────────────────────────
// Shared type glue
// ─────────────────────────────────────────────

pub type ArcState = std::sync::Arc<AppState>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Config;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::handler::Handler;
    use axum::http::{header, Method};
    use axum::routing::any;
    use axum::Router;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_state(api_key: Option<&str>) -> ArcState {
        let cfg = Config {
            hostname: "localhost".into(),
            port: 3000,
            api_key: api_key.map(|s| s.to_string()),
            custom_header_key: "custom-header".into(),
            memoization_ttl: None,
            cors_origin: "*".into(),
            ytdlp_js_runtime: None,
            ytdlp_timeout: None,
            request_timeout: None,
        };
        Arc::new(AppState::new(&cfg))
    }

    fn test_app<H, T>(state: ArcState, handler: H) -> Router<()>
    where
        H: Handler<T, ()> + Clone + Send + Sync + 'static,
        T: 'static,
    {
        Router::new()
            .route("/", any(handler))
            .route("/health", any(plain_ok))
            .layer(axum::middleware::from_fn_with_state(state.clone(), auth_mw))
            .layer(axum::middleware::from_fn_with_state(state.clone(), cors_mw))
            .layer(axum::middleware::from_fn(compress_mw))
    }

    async fn plain_ok(_req: Request) -> Response {
        Response::new(Body::from("ok"))
    }

    async fn big_json(_req: Request) -> Response {
        let body = format!("{{\"data\":\"{}\"}}", "x".repeat(2000));
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    }

    #[tokio::test]
    async fn cors_preflight_204_with_echoed_headers() {
        let app = test_app(test_state(None), plain_ok);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/")
                    .header("Origin", "http://example.com")
                    .header("Access-Control-Request-Method", "GET")
                    .header("Access-Control-Request-Headers", "content-type, x-custom")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                .unwrap(),
            "GET,POST,OPTIONS"
        );
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
                .unwrap(),
            "content-type,x-custom"
        );
        assert_eq!(
            res.headers().get(header::VARY).unwrap(),
            "Access-Control-Request-Headers"
        );
        assert!(!res.headers().contains_key(header::ALLOW));
    }

    #[tokio::test]
    async fn cors_preflight_without_request_headers() {
        let app = test_app(test_state(None), plain_ok);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/")
                    .header("Origin", "http://example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(!res
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_HEADERS));
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn cors_header_on_regular_responses() {
        let app = test_app(test_state(None), plain_ok);
        let res = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn auth_disabled_without_key() {
        let app = test_app(test_state(None), plain_ok);
        let res = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_via_user_agent_substring() {
        let app = test_app(test_state(Some("secret123")), plain_ok);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::USER_AGENT, "abcsecret123def")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_missing_headers_401_realm() {
        let app = test_app(test_state(Some("secret123")), plain_ok);
        let res = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            res.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer realm=\"\""
        );
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/octet-stream"
        );
    }

    #[tokio::test]
    async fn auth_malformed_bearer_400() {
        let app = test_app(test_state(Some("secret123")), plain_ok);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, "Bearer")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            res.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer error=\"invalid_request\""
        );
    }

    #[tokio::test]
    async fn auth_wrong_token_401_invalid_token() {
        let app = test_app(test_state(Some("secret123")), plain_ok);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, "Bearer wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            res.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer error=\"invalid_token\""
        );
    }

    #[tokio::test]
    async fn auth_valid_bearer_passes() {
        let app = test_app(test_state(Some("secret123")), plain_ok);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, "Bearer secret123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_is_bypassed_on_health_endpoint() {
        // Even with an API key set and no credentials, /health must stay open.
        let app = test_app(test_state(Some("secret123")), plain_ok);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn compress_large_json_with_gzip() {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let app = test_app(test_state(None), big_json);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::ACCEPT_ENCODING, "gzip, deflate, br")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.headers().get(header::CONTENT_ENCODING).unwrap(), "gzip");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let mut decoder = GzDecoder::new(&bytes[..]);
        let mut decoded = String::new();
        decoder.read_to_string(&mut decoded).unwrap();
        assert!(decoded.starts_with("{\"data\":\""));
        assert_eq!(decoded.len(), 2011);
    }

    #[tokio::test]
    async fn compress_skips_small_bodies_and_head() {
        let app = test_app(test_state(None), plain_ok);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!res.headers().contains_key(header::CONTENT_ENCODING));

        let app = test_app(test_state(None), big_json);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri("/")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!res.headers().contains_key(header::CONTENT_ENCODING));
    }
}
