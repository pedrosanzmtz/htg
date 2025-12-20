# Build stage
FROM rust:1.83-bookworm AS builder

WORKDIR /app

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock ./
COPY htg/Cargo.toml htg/Cargo.toml
COPY htg-service/Cargo.toml htg-service/Cargo.toml

# Create dummy source files to build dependencies
RUN mkdir -p htg/src htg-service/src && \
    echo "pub fn dummy() {}" > htg/src/lib.rs && \
    echo "fn main() {}" > htg-service/src/main.rs

# Build dependencies only (this layer will be cached)
RUN cargo build --release -p htg-service && \
    rm -rf htg/src htg-service/src

# Copy actual source code
COPY htg/src htg/src
COPY htg-service/src htg-service/src

# Touch files to update timestamps and rebuild
RUN touch htg/src/lib.rs htg-service/src/main.rs

# Build the actual application
RUN cargo build --release -p htg-service

# Runtime stage
FROM debian:bookworm-slim

# Install SSL certificates for HTTPS downloads
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/htg-service /app/htg-service

# Create data directory
RUN mkdir -p /data/srtm

# Default environment variables
ENV HTG_DATA_DIR=/data/srtm
ENV HTG_CACHE_SIZE=100
ENV HTG_PORT=8080
ENV RUST_LOG=htg_service=info,tower_http=info

# Expose the HTTP port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run the service
CMD ["/app/htg-service"]
