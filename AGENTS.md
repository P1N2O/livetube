# LiveTube Project Documentation

## Project Overview

**LiveTube** is a lightweight web server that resolves live stream URLs to HLS (HTTP Live Streaming) format. It provides a simple API for accessing YouTube live streams programmatically.

- **Repository**: https://github.com/P1N2O/livetube
- **Author**: Manuel Pinto (manuel@pinto.dev)
- **License**: MIT

## Tech Stack

- **Runtime**: Bun
- **Framework**: Hono (web framework)
- **Language**: TypeScript
- **YouTube API**: youtubei.js (Innertube)
- **PO Token Generator**: youtube-po-token-generator

## Key Dependencies

| Package                    | Purpose                      |
| -------------------------- | ---------------------------- |
| hono                       | Web framework                |
| youtubei.js                | YouTube data fetching        |
| youtube-po-token-generator | Generate YouTube PO tokens   |
| @std/cache                 | TTL-based caching            |
| @std/datetime              | Date/time utilities          |
| @std/fmt                   | Formatting utilities         |
| user-agents                | User-Agent string generation |

## Project Structure

```
.
├── index.ts          # Main application entry point
├── build.ts          # Build script for cross-platform binaries
├── package.json      # Package manifest
├── tsconfig.json     # TypeScript configuration
├── Dockerfile        # Multi-stage Docker build
├── docker-compose.yml
├── README.md
├── LICENSE
└── .env              # Environment configuration
```

## Commands

| Command         | Description                        |
| --------------- | ---------------------------------- |
| `bun run dev`   | Run in development mode with watch |
| `bun run build` | Compile binaries for all platforms |
| `bun run taze`  | Update dependencies                |

## Environment Variables

| Variable        | Default       | Description                         |
| --------------- | ------------- | ----------------------------------- |
| HOSTNAME        | hostname      | Server binding host                 |
| PORT            | 3000          | Server port                         |
| API_KEY         | -             | Optional API key for authentication |
| MEMOIZATION_TTL | 30            | Cache TTL in minutes (0 to disable) |
| CUSTOM_X_HEADER | custom-header | Custom header parameter name        |
| COOKIE          | -             | YouTube cookie                      |
| PO_TOKEN        | -             | YouTube PO token                    |
| VISITOR_DATA    | -             | YouTube visitor data                |
| CORS_ORIGIN     | \*            | Allowed CORS origins                |

## API Endpoints

### GET /

Main endpoint for stream resolution.

**Query Parameters:**

- `v`: YouTube video ID
- `c`: YouTube channel handle
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
5. **Docker Support**: Multi-arch Docker images.
6. **API Key Protection**: Optional Bearer token or User-Agent authentication.
7. **CORS Support**: Configurable cross-origin resource sharing.

## Authentication

If `API_KEY` is set, requests must include:

- Header: `Authorization: Bearer YOUR_API_KEY`
- Or User-Agent containing the API key.

## Build Targets

The build script compiles binaries for:

- Linux x64
- Linux ARM64
- macOS x64
- macOS ARM64
- Windows x64

## CI/CD

GitHub Actions workflow (`.github/workflows/release.yml`):

- Triggers on version tags (`v*.*.*`)
- Builds binaries for all platforms
- Builds and pushes Docker images to GHCR
- Creates GitHub releases with artifacts

## Development Notes

- Uses Bun as the runtime and build tool
- Strict TypeScript configuration
- Memoization via @std/cache for stream URLs
- YouTube session management with Innertube
- Non-root user in Docker for security
