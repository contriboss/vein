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

# Build for native architecture
RUN cargo build --release

# Stage 2: Runtime - Distroless
FROM gcr.io/distroless/cc-debian12:nonroot

# Copy binary from builder
COPY --from=builder /build/target/release/vein /usr/local/bin/vein

# Set working directory
WORKDIR /data

# Expose port
EXPOSE 8346

# Container healthcheck hitting Vein's liveness endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=45s --retries=3 \
    CMD ["/usr/local/bin/vein", "health"]

# Run vein
ENTRYPOINT ["/usr/local/bin/vein"]
CMD ["serve"]
