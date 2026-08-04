# syntax=docker/dockerfile:1

# ─────────────────────────────────────────────
# Runtime: the original oven/bun:alpine base — bun (yt-dlp's JS runtime) is
# already present at /usr/local/bin/bun, and this is the exact image the
# original project shipped on.
#
# The livetube binary is NOT compiled here. CI cross-compiles it with
# cargo-zigbuild (native/fast, no QEMU) and feeds it via the build context
# as dist/livetube-<TARGETARCH>. For a standalone `docker build`, run
# `script/build.sh` first (or drop a binary at dist/livetube-amd64).
# ─────────────────────────────────────────────
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

# Non-root user for security; /cache is the default cache root (CACHE_DIR),
# writable by the livetube user.
RUN addgroup -S livetube && adduser -S livetube -G livetube \
 && mkdir -p /cache \
 && chown livetube:livetube /cache

# TARGETARCH is "amd64" | "arm64" — the prebuilt binary is named dist/livetube-<TARGETARCH>.
# Installed as root, before dropping privileges.
ARG TARGETARCH
COPY dist/livetube-${TARGETARCH} /usr/local/bin/livetube
RUN chmod +x /usr/local/bin/livetube

USER livetube

ENV PORT=3000
ENV CACHE_DIR=/cache
EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/livetube"]

HEALTHCHECK --start-period=10s --interval=60s --timeout=5s --retries=3 \
  CMD wget -qO- http://${HOST:-$HOSTNAME}:${PORT:-3000}/health || exit 1
