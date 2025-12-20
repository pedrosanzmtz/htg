# Build stage
FROM rust:1.83-bookworm AS builder

WORKDIR /app

# Copy everything needed for the build
COPY Cargo.toml Cargo.lock ./
COPY htg htg
COPY htg-service htg-service

# Build the application
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
