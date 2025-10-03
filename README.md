# LiveTube

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Deno Version](https://img.shields.io/badge/deno-2.5+-blue.svg)](https://deno.com/blog/v2.5)

Generate HSL live streams with fallback support. LiveTube is a lightweight web
server that resolves live stream URLs to HLS (HTTP Live Streaming) format,
providing a simple API for accessing live streams programmatically.

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

### From Source (Deno)

1. **Prerequisites**: Install [Deno](https://deno.com/) (version 2.5 or higher).

2. **Clone the Repository**:
   ```bash
   git clone https://github.com/p1n2o/livetube.git
   cd livetube
   ```

3. **Install Dependencies**:
   ```bash
   deno install
   ```

4. **Run in Development Mode**:
   ```bash
   deno run dev
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

- **[Linux x86_64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-x86_64-unknown-linux-gnu)**: `livetube-x86_64-unknown-linux-gnu`
- **[Linux ARM64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-aarch64-unknown-linux-gnu)**: `livetube-aarch64-unknown-linux-gnu`
- **[macOS x86_64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-x86_64-apple-darwin)**: `livetube-x86_64-apple-darwin`
- **[macOS ARM64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-aarch64-apple-darwin)**: `livetube-aarch64-apple-darwin`
- **[Windows x86_64](https://github.com/P1N2O/livetube/releases/latest/download/livetube-x86_64-pc-windows-msvc.exe)**: `livetube-x86_64-pc-windows-msvc.exe`

Make the binary executable (Linux/macOS):

```bash
chmod +x livetube-linux-x86_64
./livetube-linux-x86_64
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
    image: p1n2o/livetube:latest
    container_name: livetube
    environment:
      HOST: "0.0.0.0" # Binding host (Default: 127.0.0.1)
      PORT: "3000" # Port inside container (Default: 3000)
      API_KEY: "your-api-key" # Optional: set your API key for auth (passed as Bearer token or User-Agent header)
      CACHE_TTL: "30" # Cache TTL in minutes (Default: 30)
      CACHE_DIR: "/data/cache" # Path for cache (Default: ./.cache)
      CORS_ORIGIN: "*" # Allowed CORS origins (Default: *)
    ports:
      - "3000:3000"
    healthcheck:
      test: ["CMD", "./livetube", "--health"]
      start_period: 10s
      interval: 60s
      timeout: 10s
      retries: 3
    volumes:
      - ./data/cache:/data/cache # Persist cached routes across restarts
    restart: unless-stopped
```

### Deployment and Docker Hub

#### Automated Builds

To set up automated Docker Hub builds:

1. Link your GitHub repository to Docker Hub.
2. Enable automated builds in Docker Hub settings.
3. Configure the build to use the `Dockerfile` in the root directory.
4. Set build tags to include version tags (e.g., `v{version}` from `deno.json`).

#### Repository Description

Docker Hub can automatically use this README.md as the repository overview if:

- The repository is linked to a GitHub repo containing this file.
- In Docker Hub settings, enable "Repository Links" to sync descriptions.

For manual setup:

1. Go to your Docker Hub repository settings.
2. Under "Repository Settings" > "Repository Links", add the GitHub repository
   URL.
3. The full description from README.md will be displayed on the Docker Hub page.

## Usage

### Basic API Usage

1. **Start the Server**:
   ```bash
   deno task dev  # or run the binary/Docker container
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
   deno install
   ```

2. **Development Tasks**:
   - `deno task dev`: Run in watch mode
   - `deno task format`: Format code
   - `deno task lint`: Lint code
   - `deno task compile`: Compile to binary
   - `deno task compile:all`: Compile for all platforms

### Building

Run the build script to create binaries and Docker images:

```bash
./build.sh
```

This will:

- Compile executables for multiple platforms
- Build and tag Docker images with version from `deno.json`

### Project Structure

```
.
├── main.ts          # Main application entry point
├── deno.json        # Deno configuration and version
├── deno.lock        # Dependency lock file
├── build.sh         # Build script for binaries and Docker
├── Dockerfile       # Multi-stage Docker build
├── README.md        # This file
└── .vscode/         # VS Code settings
```

### Contributing

Contributions are welcome! Please follow these steps:

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes
4. Run tests and lint: `deno task lint && deno task format`
5. Commit changes: `git commit -m 'Add amazing feature'`
6. Push to branch: `git push origin feature/amazing-feature`
7. Open a Pull Request

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

## Support

For support, email manuel@pinto.dev or open an issue on GitHub.

---

**Note**: This tool interacts with YouTube's API and is subject to their terms
of service. Use responsibly.
