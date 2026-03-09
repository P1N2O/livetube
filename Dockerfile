FROM oven/bun:alpine AS deps
WORKDIR /app
COPY package.json .npmrc bun.lock ./
RUN bun install --frozen-lockfile --production

FROM oven/bun:alpine
WORKDIR /app
LABEL org.opencontainers.image.source="https://github.com/P1N2O/livetube"

# Install yt-dlp dynamically based on architecture
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

# Non-root user for security
RUN addgroup -S livetube && adduser -S livetube -G livetube \
  && mkdir -p .cache \
  && chown -R livetube:livetube /app
USER livetube

COPY --from=deps /app/node_modules ./node_modules
COPY index.ts ./

ENV PORT=3000
EXPOSE 3000

ENTRYPOINT ["bun", "run", "index.ts"]

HEALTHCHECK --start-period=10s --interval=60s --timeout=5s --retries=3 \
  CMD wget -qO- http://${HOST:-$HOSTNAME}:${PORT:-3000}/health || exit 1