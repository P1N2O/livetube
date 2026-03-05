# LiveTube

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Generate HSL live streams with fallback support. LiveTube is a lightweight web server that resolves live stream URLs to HLS (HTTP Live Streaming) format, providing a simple API for accessing live streams programmatically.

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

## Installation

### From Source (Bun)

1. **Prerequisites**: Install [Bun](https://bun.com/).

2. **Clone the Repository**:

   ```bash
   git clone https://github.com/p1n2o/livetube.git
   cd livetube
   ```

3. **Install Dependencies**:

   ```bash
   bun install
   ```

4. **Run in Development Mode**:

   ```bash
   bun run dev
   ```

   Open [http://localhost:3000](http://localhost:3000) in your browser.

5. **Access Streams**:
   ```bash
   curl "http://localhost:3000?v=vYRfQo6JMxc&c=@unitednations&v=dQw4w9WgXcQ"
   ```
   This will redirect to the HLS manifest URL if the stream is live
   [vYRfQo6JMxc](https://www.youtube.com/watch?v=vYRfQo6JMxc), else it will try
   to resolve the next source
   [@unitednations](https://www.youtube.com/@unitednations/live).

### Using Pre-compiled Binaries

Download the latest release from the
[releases page](https://github.com/p1n2o/livetube/releases) and run the
appropriate binary for your platform:

- **[Linux x64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-linux-x64)**: `livetube-linux-x64`
- **[Linux ARM64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-linux-arm64)**: `livetube-linux-arm64`
- **[macOS x64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-macos-x64)**: `livetube-macos-x64`
- **[macOS ARM64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-macos-arm64)**: `livetube-macos-arm64`
- **[Windows x86_64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-windows-x64.exe)**: `livetube-windows-x64.exe`

Make the binary executable (Linux/macOS):

```bash
chmod +x livetube-linux-x64 \
./livetube-linux-x64
```

### Using Docker

Pull and run the Docker image:

```bash
docker run -d \
  --name livetube \
  -p 3000:3000 \
  p1n2o/livetube:latest
```

Or use Docker Compose:

```yaml
services:
  livetube:
    image: ghcr.io/p1n2o/livetube:latest
    container_name: livetube
    ports:
      - "3000:3000"
    volumes:
      - ./data/cache:/app/cache # Persist YT session and cached routes across restarts
    restart: unless-stopped
```

## Usage

### Basic API Usage

1. **Start the Server**:

   ```bash
   bun run dev  # or run the binary/Docker container
   ```

2. **Access Streams**:
   - **By Video ID**: `http://localhost:3000?v=VIDEO_ID`
   - **By Channel URL**: `http://localhost:3000?c=CHANNEL_HANDLE`
   - **Direct HLS URL**: `http://localhost:3000?x=HLS_URL`
   - **Multiple Streams**:
     `http://localhost:3000?v=VIDEO_ID&c=CHANNEL_HANDLE&x=HLS_URL`

   Example:

   ```bash
   curl "http://localhost:3000?v=dQw4w9WgXcQ"
   ```

   This will redirect to the HLS manifest URL if the stream is live.

3. **Health Check**:

   ```bash
   curl http://localhost:3000/health
   ```

4. **Clear Cache** (if authenticated):
   ```bash
   curl http://localhost:3000/clear-cache
   ```

### Configuration

Set environment variables to configure the server:

- `HOST`: Binding host (default: `127.0.0.1`)
- `PORT`: Port number (default: `3000`)
- `API_KEY`: Optional API key for authentication (Bearer token or User-Agent
  substring)
- `CACHE_TTL`: Cache time-to-live in minutes (default: `30`, set to `0` to
  disable)
- `CACHE_DIR`: Directory for cache storage (default: `./.cache`)
- `CORS_ORIGIN`: Allowed CORS origins (default: `*`)

### Authentication

If `API_KEY` is set, requests must include:

- Header: `Authorization: Bearer YOUR_API_KEY`
- Or User-Agent containing the API key.

## Development

### Setup

1. **Clone and Install**:

   ```bash
   git clone https://github.com/p1n2o/livetube.git
   cd livetube
   bun install
   ```

2. **Development Tasks**:
   - `bun run dev`: Run in watch mode
   - `bun run build`: Compile for all platforms

### Project Structure

```
.
├── index.ts          # Main application entry point
├── package.json      # Package manifest
├── bun.lock          # Dependency lock file
├── build.ts          # Build script for binaries
├── Dockerfile        # Multi-stage Docker build
├── README.md         # This file
└── .vscode/          # VS Code settings
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
    - `v`: YouTube video ID
    - `c`: YouTube channel handle
    - `x`: Direct HLS URL for validation
- `GET /health` - Health check endpoint
- `GET /clear-cache` - Clear the cache (requires auth if API_KEY is set)

### Response Codes

- `200`: Success (redirects to HLS URL or returns status)
- `302`: Redirect to HLS manifest
- `401`: Unauthorized (missing API key)
- `404`: No valid stream found

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file
for details.

---
