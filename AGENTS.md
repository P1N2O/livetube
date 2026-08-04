# LiveTube Project Documentation

## Project Overview

**LiveTube** is a lightweight web server that resolves live stream URLs to HLS (HTTP Live Streaming) format. It provides a simple API for accessing live streams programmatically.

- **Repository**: https://github.com/P1N2O/livetube
- **Author**: Manuel Pinto (manuel@pinto.dev)
- **License**: MIT

## Tech Stack

- **Runtime**: Rust (stable, edition 2021)
- **Framework**: axum (web framework on tokio)
- **Language**: Rust
- **YT-DLP**: yt-dlp (external binary, spawned per lookup)

## Key Dependencies

|Crate|Purpose|
|---|---|
|axum|Web framework|
|tokio|Async runtime|
|reqwest|Stream validation requests (rustls TLS)|
|serde_json|JSON responses|
|flate2|gzip/deflate response compression|
|url / percent-encoding|URL and query parsing|
|chrono|Startup banner date formatting|

## Project Structure

```
.
├── src/              # Rust source
│   ├── main.rs       # Entry point, routing, query-param collection
│   ├── state.rs      # Environment configuration and shared state
│   ├── middleware.rs # Logger, compression, CORS, auth (hono-compatible)
│   ├── resolvers.rs  # Stream validation and yt-dlp extraction
│   ├── cache.rs      # TTL cache with in-flight deduplication
│   ├── params.rs     # URLSearchParams-compatible query parsing
│   └── fmt.rs        # Output formatting (@std/fmt-compatible)
├── Cargo.toml        # Package manifest
├── Dockerfile        # Runtime image (bun base) packaging prebuilt binaries
├── compose.yaml      # Docker Compose configuration
├── README.md         # Project documentation
├── LICENSE           # License file
├── .env.example      # Environment variable reference
└── script            # Utility scripts
    ├── build.sh      # Cross-compile all release binaries (cargo-zigbuild)
    └── release.sh    # Create and push a release tag
```

## Commands

|Command|Description|
|---|---|
|`cargo run`|Run the server in development|
|`cargo test`|Run the unit test suite|
|`cargo clippy`|Lint|
|`cargo fmt`|Format|
|`./script/build.sh`|Cross-compile release binaries for all platforms|
|`./script/release.sh vX.Y.Z`|Tag and push a GitHub release|

## Environment Variables

|Variable|Default|Description|
|---|---|---|
|HOSTNAME|localhost|Server binding host (`HOST` accepted as alias)|
|PORT|3000|Server port|
|API_KEY|-|Optional API key for authentication|
|MEMOIZATION_TTL|30|Cache TTL in minutes (0 to disable)|
|CUSTOM_X_HEADER|custom-header|Custom header parameter name|
|CORS_ORIGIN|\*|Allowed CORS origins|
|YTDLP_JS_RUNTIME|auto|yt-dlp `--js-runtimes` value (auto-detects bun)|
|YTDLP_TIMEOUT|0|Kill yt-dlp after N seconds (0 = disabled)|
|REQUEST_TIMEOUT|0|Stream validation timeout in seconds (0 = disabled)|

## API Endpoints

### GET /

Main endpoint for stream resolution.

**Query Parameters:**

- `v`: YT video ID
- `c`: YT channel handle
- `x`: Direct HLS URL for validation

**Examples:**

```
GET /?v=VIDEO_ID
GET /?c=CHANNEL_HANDLE
GET /?x=HLS_URL
GET /?v=VIDEO_ID&c=CHANNEL_HANDLE&x=HLS_URL
```

### GET /health

Health check endpoint. Returns `{"status": "ok"}`.

## Features

1. **HLS Stream Resolution**: Automatically resolves video IDs or channel URLs to HLS manifest URLs.
2. **Fallback Support**: Validates streams and provides fallbacks if primary sources fail.
3. **Caching**: TTL-based memoization to reduce API calls.
4. **Multi-Platform Binaries**: Pre-compiled executables for Linux, macOS, and Windows.
5. **Docker Support**: Multi-arch Docker images (static musl binaries, alpine runtime).
6. **API Key Protection**: Optional Bearer token or User-Agent authentication.
7. **CORS Support**: Configurable cross-origin resource sharing.

## Authentication

If `API_KEY` is set, requests must include:

- Header: `Authorization: Bearer YOUR_API_KEY`
- Or User-Agent containing the API key.

## Build Targets

The build script (and CI) compiles binaries for:

- Linux x64 (static musl)
- Linux ARM64 (static musl)
- macOS x64
- macOS ARM64
- Windows x64

## CI/CD

GitHub Actions workflow (`.github/workflows/release.yml`) — triggers on version tags (`v*.*.*`):

- Runs tests and clippy
- Cross-compiles static binaries for all platforms (cargo-zigbuild)
- Compresses with UPX
- Creates GitHub releases with artifacts
- Builds and pushes multi-arch Docker images to GHCR

## Development Notes

- The port is 1:1 behavior-compatible with the original Bun implementation:
  middleware order (logger → compress → cors → auth), response headers,
  log lines, caching keys and TTL semantics were captured empirically from the
  original server and locked in with unit tests (`middleware.rs` tests).
- yt-dlp is spawned with the same flags as the original (`--quiet
  --no-warnings --no-progress --no-update -f best -g` plus
  `--js-runtimes bun:/usr/local/bin/bun` when bun is present).
- Non-root user in Docker for security.
