# Stage 1: Generate recipe.json for dependency caching
FROM lukemathwalker/cargo-chef:latest-rust-1.83-bookworm AS chef
WORKDIR /app

# Stage 2: Plan dependencies
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies (cached layer)
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this layer is cached until Cargo.toml/Cargo.lock change
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source and build application
COPY Cargo.toml Cargo.lock ./
COPY htg htg
COPY htg-service htg-service
RUN cargo build --release -p htg-service

# Stage 4: Runtime
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
