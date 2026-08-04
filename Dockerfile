# syntax=docker/dockerfile:1

# ─────────────────────────────────────────────
# Builder: static musl binaries (rustls, no C deps)
# ─────────────────────────────────────────────
FROM rust:latest AS builder
WORKDIR /app

RUN apk add --no-cache musl-dev

COPY Cargo.toml Cargo.lock ./
COPY src ./src

ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry \
     case "$TARGETARCH" in \
     amd64)  TARGET="x86_64-unknown-linux-musl" ;; \
     arm64)  TARGET="aarch64-unknown-linux-musl" ;; \
     *) echo "Unsupported architecture: $TARGETARCH" && exit 1 ;; \
     esac \
     && rustup target add "$TARGET" \
     && cargo build --release --target "$TARGET" --bin livetube \
     && cp "target/$TARGET/release/livetube" /livetube

# ─────────────────────────────────────────────
# Runtime: alpine + static yt-dlp + bun (JS runtime for yt-dlp)
# ─────────────────────────────────────────────
FROM alpine:latest
WORKDIR /app
LABEL org.opencontainers.image.source="https://github.com/P1N2O/livetube"

# Install yt-dlp and bun dynamically based on architecture (same pair of
# binaries the original image shipped: yt-dlp static + bun at /usr/local/bin).
# libstdc++ is required by the bun binary.
RUN apk add --no-cache curl unzip libstdc++ \
     && ARCH="$(apk --print-arch)" \
     && case "$ARCH" in \
     x86_64)  YT_ARCH="yt-dlp_musllinux";     BUN_ARCH="bun-linux-x64-musl" ;; \
     aarch64) YT_ARCH="yt-dlp_musllinux_aarch64"; BUN_ARCH="bun-linux-aarch64-musl" ;; \
     *) echo "Unsupported architecture: $ARCH" && exit 1 ;; \
     esac \
     && curl -L "https://github.com/yt-dlp/yt-dlp/releases/latest/download/${YT_ARCH}" \
     -o /usr/local/bin/yt-dlp \
     && chmod +x /usr/local/bin/yt-dlp \
     && curl -L "https://github.com/oven-sh/bun/releases/latest/download/${BUN_ARCH}.zip" \
     -o /tmp/bun.zip \
     && unzip -o /tmp/bun.zip -d /tmp/bunpkg \
     && mv /tmp/bunpkg/*/bun /usr/local/bin/bun \
     && chmod +x /usr/local/bin/bun \
     && rm -rf /tmp/bun.zip /tmp/bunpkg

# Non-root user for security
RUN addgroup -S livetube && adduser -S livetube -G livetube
USER livetube

COPY --from=builder /livetube /usr/local/bin/livetube

ENV PORT=3000
EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/livetube"]

HEALTHCHECK --start-period=10s --interval=60s --timeout=5s --retries=3 \
     CMD wget -qO- http://${HOST:-$HOSTNAME}:${PORT:-3000}/health || exit 1
