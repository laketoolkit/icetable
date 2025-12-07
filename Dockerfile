# =============================================================================
# icetable Dockerfile
# Multi-stage build for minimal production image
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Build
# -----------------------------------------------------------------------------
FROM rust:1.83-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create dummy source to build dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    echo "pub fn lib() {}" > src/lib.rs

# Build dependencies only (cached layer)
RUN cargo build --release && rm -rf src

# Copy actual source
COPY src ./src
COPY tests ./tests
COPY fixtures ./fixtures

# Touch to invalidate cached main.rs
RUN touch src/main.rs src/lib.rs

# Build release binary
RUN cargo build --release --all-features

# Strip binary for smaller size
RUN strip target/release/icetable

# -----------------------------------------------------------------------------
# Stage 2: Runtime (Debian slim)
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 icetable

# Copy binary from builder
COPY --from=builder /app/target/release/icetable /usr/local/bin/icetable

# Set permissions
RUN chmod +x /usr/local/bin/icetable

# Switch to non-root user
USER icetable
WORKDIR /home/icetable

# Create config directory
RUN mkdir -p /home/icetable/.config/icetable

# Default environment variables
ENV RUST_LOG=warn
ENV ICETABLE_MAX_MEMORY=0
ENV ICETABLE_TIMEOUT=0

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD icetable --version || exit 1

ENTRYPOINT ["icetable"]
CMD ["--help"]

# -----------------------------------------------------------------------------
# Stage 3: Alpine variant (smaller but may have compatibility issues)
# -----------------------------------------------------------------------------
FROM alpine:3.19 AS alpine

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    libgcc \
    libstdc++

# Create non-root user
RUN adduser -D -u 1000 icetable

# Copy binary (needs musl build - use runtime stage for glibc)
# Note: For Alpine, you need to build with musl target
# This stage is a placeholder - use 'runtime' stage for production
COPY --from=builder /app/target/release/icetable /usr/local/bin/icetable

USER icetable
WORKDIR /home/icetable

ENTRYPOINT ["icetable"]
CMD ["--help"]
