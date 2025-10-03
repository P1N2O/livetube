# Multi-stage build for optimized, small Docker images
# Build stage: Use Deno Alpine to compile the binary for multiple platforms
FROM --platform=$BUILDPLATFORM denoland/deno:alpine AS builder

# Set working directory
WORKDIR /app

# Copy project files
COPY deno.json deno.lock ./
COPY main.ts favicon.ico ./

# Compile for the target platform
ARG TARGETPLATFORM
RUN if [ "$TARGETPLATFORM" = "linux/amd64" ]; then \
        deno compile -A --target x86_64-unknown-linux-gnu -o livetube main.ts; \
    elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
        deno compile -A --target aarch64-unknown-linux-gnu -o livetube main.ts; \
    else \
        echo "Unsupported platform: $TARGETPLATFORM" && exit 1; \
    fi

# Runtime stage: distroless
FROM gcr.io/distroless/cc

# Add metadata labels
ARG VERSION=latest
LABEL version="$VERSION" \
      description="Generate HSL live streams with fallback support." \
      maintainer="Manuel Pinto <manuel@pinto.dev>" \
      source="https://github.com/p1n2o/livetube"

# Set working directory
WORKDIR /app

# Copy the compiled binary from builder stage
COPY --from=builder /app/livetube .

# Expose port (default 3000, can be overridden with env)
# EXPOSE 3000

# Run the binary
CMD ["./livetube"]