# Multi-stage build for Vein RubyGems proxy server

# Stage 1: Builder
FROM rust:1.92-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    cmake \
    clang \
    llvm-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace sources
COPY Cargo.toml ./
COPY src ./src
COPY crates ./crates

# Build for native architecture with PostgreSQL support
RUN cargo build --release --no-default-features --features tls,postgres

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    zlib1g \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 1000 -m vein

# Copy binary from builder
COPY --from=builder /build/target/release/vein /usr/local/bin/vein

# Set working directory and permissions
WORKDIR /data
RUN chown vein:vein /data

USER vein

# Expose port
EXPOSE 8346

# Container healthcheck hitting Vein's liveness endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS http://localhost:8346/up || exit 1

# Run vein
ENTRYPOINT ["/usr/local/bin/vein"]
CMD ["serve"]
