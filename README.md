# LiveTube

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Generate HLS live streams with fallback support. LiveTube is a lightweight web
server that resolves live stream URLs to HLS (HTTP Live Streaming) format using
[yt-dlp](https://github.com/yt-dlp/yt-dlp), providing a simple API for
accessing live streams programmatically.

> **Rust port**: this project is a 1:1 Rust port of the original Bun/Hono
> implementation — same endpoints, headers, caching semantics and log output,
> compiled to a single static binary. It is a drop-in replacement for the
> previous binaries, Docker image and release workflow.

## Features

- **HLS Stream Resolution**: Automatically resolves video IDs or channel URLs to
  HLS manifest URLs.
- **Fallback Support**: Validates streams and provides fallbacks if primary
  sources fail.
- **Caching**: Optional TTL-based caching to reduce API calls and improve
  performance.
- **Multi-Platform Binaries**: Pre-compiled executables for Linux, macOS, and
  Windows along with Docker support.
- **API Key Protection**: Optional authentication for securing endpoints.
- **CORS Support**: Configurable cross-origin resource sharing.
- **Single Static Binary**: No runtime dependencies on Linux (musl), built with
  `rustls` (no OpenSSL).

## Installation

### 1. Docker

Pull and run the Docker image:

```bash
docker run -d \
  --name livetube \
  -p 3000:3000 \
  ghcr.io/p1n2o/livetube:latest
```

Or use Docker Compose:

```yaml
services:
  livetube:
    image: ghcr.io/p1n2o/livetube:latest
    container_name: livetube
    ports:
      - "3000:3000"
    restart: unless-stopped
    environment:
      CACHE_DIR: /home/livetube/.cache/livetube
    volumes:
      - livetube-cache:/home/livetube/.cache/livetube

volumes:
  livetube-cache:
```

### 2. Pre-compiled Binaries

Download the latest release from the
[releases page](https://github.com/p1n2o/livetube/releases) and run the
appropriate binary for your platform:

- **[Linux x64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-linux-x64)**: `livetube-linux-x64`
- **[Linux ARM64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-linux-arm64)**: `livetube-linux-arm64`
- **[macOS x64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-macos-x64)**: `livetube-macos-x64`
- **[macOS ARM64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-macos-arm64)**: `livetube-macos-arm64`
- **[Windows x86_64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-windows-x64.exe)**: `livetube-windows-x64.exe`

Example (linux-x64):

```bash
curl -L https://github.com/P1N2O/livetube/releases/latest/download/livetube-linux-x64 -o livetube && chmod +x livetube && ./livetube
```

> The binaries expect `yt-dlp` on the `PATH`. The Docker image ships with
> `yt-dlp` (static) and `bun` (as yt-dlp's JS runtime) pre-installed.

### 3. Source (Rust)

1. **Prerequisites**: Install [Rust](https://rustup.rs/) (stable).

2. **Clone the Repository**:

   ```bash
   git clone https://github.com/p1n2o/livetube.git && cd livetube
   ```

3. **Build and Run**:

   ```bash
   cargo build --release
   ./target/release/livetube
   ```

   To build the Docker image locally, cross-compile the binaries first (the
   image packages `dist/livetube-amd64` / `dist/livetube-arm64` rather than
   compiling inside Docker):

   ```bash
   ./script/build.sh        # builds all targets, not just linux
   docker build --build-arg TARGETARCH=amd64 -t livetube .
   ```

   To cross-compile all release binaries (requires
   [zig](https://ziglang.org/) and
   [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild)):

   ```bash
   ./script/build.sh
   ```

## Usage

### Basic API Usage

- **By Video ID**: `http://localhost:3000?v=VIDEO_ID`
- **By Channel URL**: `http://localhost:3000?c=CHANNEL_HANDLE`
- **Direct HLS URL**: `http://localhost:3000?x=HLS_URL`
- **Multiple Streams with fallback**:
  `http://localhost:3000?v=VIDEO_ID&c=CHANNEL_HANDLE&x=HLS_URL`

Example:

```bash
curl "http://localhost:3000?v=dQw4w9WgXcQ"
```

This will redirect to the HLS manifest URL if the stream is live.

**Health Check**:

```bash
curl http://localhost:3000/health
```

### Configuration

Set environment variables to configure the server:

- `HOSTNAME`: Binding host (default: `localhost`; `HOST` is accepted as an
  alias)
- `PORT`: Port number (default: `3000`)
- `API_KEY`: Optional API key for authentication (Bearer token or User-Agent
  substring)
- `MEMOIZATION_TTL`: Cache time-to-live for successful lookups in minutes
  (default: `30`, set to `0` to disable)
- `CUSTOM_X_HEADER`: Custom header key to pass to `x` requests
- `CORS_ORIGIN`: Allowed CORS origins (default: `*`)

Caching extras (not present in the original Bun version):

- `CACHE_NEGATIVE_TTL`: How long failed lookups are cached, in minutes
  (default: `2`, set to `0` to disable). Dead streams recover quickly instead
  of being memoized for the full TTL.
- `CACHE_DIR`: Shared cache root (default: `~/.cache/livetube`). Drives both
  the yt-dlp web cache (`--cache-dir`) and a persistent redb database
  (`$CACHE_DIR/redb/cache.redb`) that survives restarts. Set to an empty
  string to keep everything in memory.
- `CACHE_REDIS_URL`: Optional Redis URL (e.g. `redis://localhost:6379/0`) for
  a shared cache across multiple instances (overrides the redb backend;
  entries expire via SETEX).

Other extras:

- `YTDLP_JS_RUNTIME`: yt-dlp `--js-runtimes` value, e.g.
  `bun:/usr/local/bin/bun`. Defaults to `bun:/usr/local/bin/bun` when that
  binary exists, otherwise the flag is omitted and yt-dlp falls back to its
  bundled interpreter. Set to an empty string to disable.
- `YTDLP_TIMEOUT`: Kill yt-dlp after N seconds (default: `0` = disabled).
- `REQUEST_TIMEOUT`: Timeout for direct-URL validation requests in seconds
  (default: `0` = disabled).

### Caching

Successful resolutions are memoized for `MEMOIZATION_TTL` (default 30
minutes), capped by the stream URL's own `expire` token when present so dead
manifests are never served. Failed lookups are cached for only
`CACHE_NEGATIVE_TTL` (default 2 minutes). Results live in memory (with
in-flight deduplication) and are optionally persisted:

- **redb** (default, under `CACHE_DIR`) — survives restarts; single instance
  per database file;
- **Redis** (`CACHE_REDIS_URL`) — shared across instances; recommended when
  running more than one replica.

The Docker Compose file mounts a named volume at `CACHE_DIR` so the cache
(redb + yt-dlp web cache) persists across container recreates.

### Authentication

If `API_KEY` is set, requests must include:

- Header: `Authorization: Bearer YOUR_API_KEY`
- Or User-Agent containing the API key.

## Development

### Setup

1. **Clone and Install**:

   ```bash
   git clone https://github.com/p1n2o/livetube.git && cd livetube
   ```

2. **Development Tasks**:
   - `cargo run`: Run the server (with `RUST_LOG`-free console logging)
   - `cargo test`: Run the unit test suite
   - `cargo clippy`: Lint
   - `cargo fmt`: Format

### Project Structure

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
├── script/
│   ├── build.sh      # Cross-compile all release binaries
│   └── release.sh    # Create and push a release tag
├── .github/
│   ├── workflows/
│   │   └── release.yml  # Tag-triggered release pipeline (binaries + image)
│   └── dependabot.yml   # Cargo + GitHub Actions updates
└── README.md         # This file
```

### Contributing

Contributions are welcome! Please follow these steps:

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes
4. Commit changes: `git commit -m 'Add amazing feature'`
5. Push to branch: `git push origin feature/amazing-feature`
6. Open a Pull Request

### Reporting Issues

Report bugs or request features via
[GitHub Issues](https://github.com/p1n2o/livetube/issues).

## API Reference

### Endpoints

- `GET /` - Main endpoint for stream resolution
  - Query Parameters:
    - `v`: YT video ID
    - `c`: YT channel handle
    - `x`: Direct HLS URL for validation
- `GET /health` - Health check endpoint

### Response Codes

- `200`: Success (redirects to HLS URL or returns status)
- `302`: Redirect to HLS manifest
- `401`: Unauthorized (missing API key)
- `404`: No valid stream found

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file
for details.
