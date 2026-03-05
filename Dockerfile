FROM oven/bun:alpine AS deps
WORKDIR /app
COPY package.json bun.lockb ./
RUN bun install --frozen-lockfile --production

FROM oven/bun:alpine
WORKDIR /app

# Non-root user for security
RUN addgroup -S livetube && adduser -S livetube -G livetube
USER livetube

COPY --from=deps /app/node_modules ./node_modules
COPY index.ts ./

ENV PORT=3000
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD bun run index.ts --health

ENTRYPOINT ["bun", "run", "index.ts"]