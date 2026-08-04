//! LiveTube — lightweight web server that resolves live stream URLs to HLS
//! format. Rust port of the original Bun/Hono implementation, 1:1 feature
//! compatible (same endpoints, headers, logging and caching semantics).

mod cache;
mod fmt;
mod middleware;
mod params;
mod resolvers;
mod state;

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use serde_json::json;
use tokio::net::TcpListener;

use crate::middleware::{auth_mw, compress_mw, cors_mw, logger_mw, ArcState};
use crate::params::parse_query;
use crate::resolvers::{current_rss_bytes, resolve_hls, Param, ParamKind};
use crate::state::{AppState, Config};

// ─────────────────────────────────────────────
// Query parameter handling (JS URLSearchParams semantics)
// ─────────────────────────────────────────────

const ALLOWED_PARAMS: [&str; 3] = ["v", "c", "x"];

/// Port of the JS param-collection loop: allowed keys become params; unknown
/// keys appearing AFTER a known param are appended to its raw value.
fn collect_params(query: &str) -> Result<Vec<Param>, ()> {
    let pairs = parse_query(query)?;
    let mut params: Vec<Param> = Vec::new();
    for (key, value) in pairs {
        if ALLOWED_PARAMS.contains(&key.as_str()) {
            params.push(Param {
                kind: match key.as_str() {
                    "v" => ParamKind::Video,
                    "c" => ParamKind::Channel,
                    _ => ParamKind::Direct,
                },
                value,
            });
        } else if let Some(last) = params.last_mut() {
            last.value.push('&');
            last.value.push_str(&key);
            last.value.push('=');
            last.value.push_str(&value);
        }
    }
    Ok(params)
}

// ─────────────────────────────────────────────
// Response helpers
// ─────────────────────────────────────────────

/// JSON body; HEAD requests get the headers but an empty body (hono strips it).
fn json_response(status: StatusCode, value: serde_json::Value, is_head: bool) -> Response {
    let body = value.to_string();
    let builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json");
    if is_head {
        builder.body(Body::empty()).expect("json response")
    } else {
        builder.body(Body::from(body)).expect("json response")
    }
}

fn redirect_response(location: String) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .body(Body::empty())
        .expect("redirect response")
}

// ─────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────

fn method_allowed(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD)
}

fn not_found_response() -> Response {
    // hono default 404 body: "404 Not Found", text/plain; charset=UTF-8.
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("404 Not Found"))
        .expect("404 response")
}

async fn health(State(state): State<ArcState>, method: Method) -> Response {
    let _ = state;
    if !method_allowed(&method) {
        return not_found_response();
    }
    json_response(
        StatusCode::OK,
        json!({"status": "ok"}),
        method == Method::HEAD,
    )
}

async fn index(State(state): State<ArcState>, method: Method, req: Request) -> Response {
    if !method_allowed(&method) {
        return not_found_response();
    }
    let is_head = method == Method::HEAD;

    let params = match collect_params(req.uri().query().unwrap_or("")) {
        Ok(p) => p,
        Err(()) => {
            // Invalid percent-encoding makes JS `new URL()` throw inside the
            // handler => hono's default 500 "Internal Server Error".
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal Server Error"))
                .expect("500 response");
        }
    };

    if !params.is_empty() {
        for param in &params {
            if let Some(stream_url) = resolve_hls(&state, param).await {
                return redirect_response(stream_url);
            }
        }
        return json_response(
            StatusCode::NOT_FOUND,
            json!({"success": false, "error": "No valid stream found!"}),
            is_head,
        );
    }

    // No (recognized) params: server info, mirroring process.memoryUsage()
    // keys and @std/fmt/bytes formatting. heap/external figures have no Rust
    // equivalent; RSS is real.
    let entries = match &state.cache {
        Some(c) => Some(c.len().await),
        None => None,
    };
    let rss = current_rss_bytes();
    let zero = 0u64;
    let body = match entries {
        Some(n) => json!({
            "message": "Server running!",
            "memory": {
                "rss": fmt::format_bytes(rss as f64),
                "heapTotal": fmt::format_bytes(zero as f64),
                "heapUsed": fmt::format_bytes(zero as f64),
                "external": fmt::format_bytes(zero as f64),
                "arrayBuffers": fmt::format_bytes(zero as f64),
            },
            "authentication": {
                "enabled": if state.api_key.is_some() { "Enabled" } else { "Disabled" },
                "header": if state.api_key.is_some() { "Authorization / User-Agent" } else { "None" },
            },
            "memoization": {
                "enabled": "Enabled",
                "entries": n,
            },
        }),
        None => json!({
            "message": "Server running!",
            "memory": {
                "rss": fmt::format_bytes(rss as f64),
                "heapTotal": fmt::format_bytes(zero as f64),
                "heapUsed": fmt::format_bytes(zero as f64),
                "external": fmt::format_bytes(zero as f64),
                "arrayBuffers": fmt::format_bytes(zero as f64),
            },
            "authentication": {
                "enabled": if state.api_key.is_some() { "Enabled" } else { "Disabled" },
                "header": if state.api_key.is_some() { "Authorization / User-Agent" } else { "None" },
            },
            "memoization": {
                "enabled": "Disabled",
            },
        }),
    };

    json_response(StatusCode::OK, body, is_head)
}

async fn not_found(_req: Request) -> Response {
    not_found_response()
}

// ─────────────────────────────────────────────
// Startup
// ─────────────────────────────────────────────

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[tokio::main]
async fn main() {
    let cfg = Config::from_env();
    let state: ArcState = Arc::new(AppState::new(&cfg));

    // Middleware order (outermost first) matches hono `every(logger, compress,
    // cors, some(auth))`; axum layers wrap outward, so apply innermost first.
    //
    // `any()` skips axum's automatic `Allow` header (hono never sends one),
    // and the handlers gate on GET/HEAD so every other method still gets the
    // plain 404 hono returns.
    let app = Router::new()
        .route("/", any(index))
        .route("/health", any(health))
        .fallback(any(not_found))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth_mw))
        .layer(axum::middleware::from_fn_with_state(state.clone(), cors_mw))
        .layer(axum::middleware::from_fn(compress_mw))
        .layer(axum::middleware::from_fn(logger_mw));

    let addr = format!("{}:{}", cfg.hostname, cfg.port);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind {addr}: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "\n🚀 Server started at {}\nListening on http://{addr}/\n",
        fmt::js_date_string()
    );
    println!(
        "🔐 Authentication: {}",
        match &cfg.api_key {
            Some(key) => format!("ENABLED ({key})"),
            None => "DISABLED".to_string(),
        }
    );
    println!(
        "💾 Memoization: {}",
        match cfg.memoization_ttl {
            Some(ttl) => format!(
                "ENABLED ({})",
                fmt::format_duration_full(ttl.as_millis() as u64)
            ),
            None => "DISABLED".to_string(),
        }
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}
