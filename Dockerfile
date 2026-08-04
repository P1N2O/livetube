FROM rust:1-alpine3.21 AS builder
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

FROM oven/bun:alpine
WORKDIR /app
LABEL org.opencontainers.image.source="https://github.com/P1N2O/livetube"

# Install yt-dlp dynamically based on architecture (same as the original).
RUN apk add --no-cache curl \
     && ARCH="$(apk --print-arch)" \
     && case "$ARCH" in \
     x86_64)  YT_ARCH="yt-dlp_musllinux" ;; \
     aarch64) YT_ARCH="yt-dlp_musllinux_aarch64" ;; \
     *) echo "Unsupported architecture: $ARCH" && exit 1 ;; \
     esac \
     && curl -L "https://github.com/yt-dlp/yt-dlp/releases/latest/download/${YT_ARCH}" \
     -o /usr/local/bin/yt-dlp \
     && chmod +x /usr/local/bin/yt-dlp

RUN addgroup -S livetube && adduser -S livetube -G livetube
USER livetube

COPY --from=builder /livetube /usr/local/bin/livetube

ENV PORT=3000
EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/livetube"]

HEALTHCHECK --start-period=10s --interval=60s --timeout=5s --retries=3 \
     CMD wget -qO- http://${HOST:-$HOSTNAME}:${PORT:-3000}/health || exit 1
