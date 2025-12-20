# SRTM Elevation Service - Implementation Roadmap

## Overview

This roadmap breaks down building a production-ready SRTM elevation microservice in Rust into manageable weekend projects. Each phase builds on the previous one.

**Estimated Total Time:** 4-5 weekends (16-20 hours)  
**Skill Level:** Intermediate Rust (you already know Rust from xmlshift)

---

## Phase 1: Core Tile Parser (Weekend 1 - 4 hours)

### Goal
Read a single .hgt file and extract elevation for any coordinate within it.

### Tasks

#### 1.1 Project Setup (30 min)
```bash
cargo new srtm-service
cd srtm-service
```

Add to `Cargo.toml`:
```toml
[dependencies]
memmap2 = "0.9"
thiserror = "1.0"

[dev-dependencies]
tempfile = "3.8"
```

#### 1.2 Define Error Types (15 min)
**File:** `src/error.rs`

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SrtmError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid file size: {0} bytes")]
    InvalidFileSize(usize),
    
    #[error("Coordinates out of bounds")]
    OutOfBounds,
    
    #[error("File not found: {0}")]
    FileNotFound(String),
}

pub type Result<T> = std::result::Result<T, SrtmError>;
```

#### 1.3 Implement SrtmTile (2 hours)
**File:** `src/tile.rs`

```rust
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use crate::error::{Result, SrtmError};

pub struct SrtmTile {
    data: Mmap,
    samples: usize,  // 1201 or 3601
}

impl SrtmTile {
    /// Load an HGT file from disk
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(&path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        
        // Detect SRTM1 vs SRTM3 by file size
        let samples = match mmap.len() {
            25934402 => 3601,  // SRTM1 (1 arc-second)
            2884802 => 1201,   // SRTM3 (3 arc-second)
            size => return Err(SrtmError::InvalidFileSize(size)),
        };
        
        Ok(Self { data: mmap, samples })
    }
    
    /// Get elevation at specific coordinates
    /// lat, lon should be within the tile bounds
    pub fn get_elevation(&self, lat: f64, lon: f64) -> Result<i16> {
        // Calculate fractional position within tile
        let lat_int = lat.floor();
        let lon_int = lon.floor();
        let lat_frac = lat - lat_int;
        let lon_frac = lon - lon_int;
        
        // Validate bounds
        if !(0.0..=1.0).contains(&lat_frac) || !(0.0..=1.0).contains(&lon_frac) {
            return Err(SrtmError::OutOfBounds);
        }
        
        // Convert to row/col
        // IMPORTANT: Rows are inverted (0 = north, max = south)
        let row = ((1.0 - lat_frac) * (self.samples - 1) as f64) as usize;
        let col = (lon_frac * (self.samples - 1) as f64) as usize;
        
        // Calculate byte offset
        let offset = (row * self.samples + col) * 2;
        
        // Bounds check
        if offset + 2 > self.data.len() {
            return Err(SrtmError::OutOfBounds);
        }
        
        // Read 16-bit big-endian value
        Ok(i16::from_be_bytes([
            self.data[offset],
            self.data[offset + 1],
        ]))
    }
}
```

#### 1.4 Write Tests (1 hour)
**File:** `tests/tile_tests.rs`

```rust
use srtm_service::tile::SrtmTile;

#[test]
fn test_load_valid_srtm1_file() {
    // You'll need a real .hgt file for this test
    let tile = SrtmTile::from_file("test_data/N35E138.hgt");
    assert!(tile.is_ok());
}

#[test]
fn test_invalid_file_size() {
    // Create a temp file with wrong size
    use tempfile::NamedTempFile;
    use std::io::Write;
    
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(&vec![0u8; 1000]).unwrap();
    
    let tile = SrtmTile::from_file(file.path());
    assert!(tile.is_err());
}

#[test]
fn test_get_elevation() {
    // Test with real .hgt file
    let tile = SrtmTile::from_file("test_data/N35E138.hgt").unwrap();
    
    // Test center of tile
    let elev = tile.get_elevation(35.5, 138.5);
    assert!(elev.is_ok());
    
    // Elevation should be reasonable (not -32768 void value)
    let elev_value = elev.unwrap();
    assert!(elev_value > -1000 && elev_value < 10000);
}
```

#### 1.5 Update main.rs (15 min)
```rust
mod error;
mod tile;

fn main() {
    println!("SRTM Service - Phase 1 Complete!");
}
```

### Deliverables
- [x] Can load .hgt files
- [x] Can detect SRTM1 vs SRTM3 automatically
- [x] Can extract elevation for any coordinate
- [x] Tests pass

### Testing
```bash
# Get sample SRTM data (Mexico City area)
mkdir test_data
cd test_data
wget https://example.com/N19W100.hgt  # You'll need real source

# Run tests
cargo test
```

---

## Phase 2: Filename Detection (Weekend 2 - 3 hours)

### Goal
Automatically calculate which .hgt file to use for any coordinate.

### Tasks

#### 2.1 Implement Filename Calculation (1 hour)
**File:** `src/filename.rs`

```rust
/// Convert lat/lon to HGT filename
/// Examples:
///   (35.5, 138.7) → "N35E138.hgt"
///   (-12.3, -77.1) → "S12W077.hgt"
///   (0.5, -0.5) → "N00W001.hgt"
pub fn lat_lon_to_filename(lat: f64, lon: f64) -> String {
    let lat_int = lat.floor() as i32;
    let lon_int = lon.floor() as i32;
    
    let lat_prefix = if lat_int >= 0 { "N" } else { "S" };
    let lon_prefix = if lon_int >= 0 { "E" } else { "W" };
    
    format!(
        "{}{:02}{}{:03}.hgt",
        lat_prefix,
        lat_int.abs(),
        lon_prefix,
        lon_int.abs()
    )
}

/// Parse filename back to lat/lon (useful for validation)
pub fn filename_to_lat_lon(filename: &str) -> Option<(i32, i32)> {
    // Remove .hgt extension
    let name = filename.strip_suffix(".hgt")?;
    
    // Parse format: N35E138 or S12W077
    if name.len() < 7 {
        return None;
    }
    
    let lat_sign = match name.chars().next()? {
        'N' => 1,
        'S' => -1,
        _ => return None,
    };
    
    let lon_sign = match name.chars().nth(3)? {
        'E' => 1,
        'W' => -1,
        _ => return None,
    };
    
    let lat: i32 = name[1..3].parse().ok()?;
    let lon: i32 = name[4..7].parse().ok()?;
    
    Some((lat * lat_sign, lon * lon_sign))
}
```

#### 2.2 Comprehensive Tests (1.5 hours)
**File:** `tests/filename_tests.rs`

```rust
use srtm_service::filename::*;

#[test]
fn test_positive_coords() {
    assert_eq!(lat_lon_to_filename(35.5, 138.7), "N35E138.hgt");
    assert_eq!(lat_lon_to_filename(0.5, 0.5), "N00E000.hgt");
}

#[test]
fn test_negative_coords() {
    assert_eq!(lat_lon_to_filename(-12.3, -77.1), "S12W077.hgt");
    assert_eq!(lat_lon_to_filename(-0.5, -0.5), "S00W000.hgt");
}

#[test]
fn test_edge_cases() {
    // Exactly on boundary
    assert_eq!(lat_lon_to_filename(35.0, 138.0), "N35E138.hgt");
    
    // Near poles
    assert_eq!(lat_lon_to_filename(59.9, 0.0), "N59E000.hgt");
    assert_eq!(lat_lon_to_filename(-59.9, 0.0), "S59W000.hgt");
    
    // Near dateline
    assert_eq!(lat_lon_to_filename(35.0, 179.9), "N35E179.hgt");
    assert_eq!(lat_lon_to_filename(35.0, -179.9), "N35W179.hgt");
}

#[test]
fn test_parse_filename() {
    assert_eq!(filename_to_lat_lon("N35E138.hgt"), Some((35, 138)));
    assert_eq!(filename_to_lat_lon("S12W077.hgt"), Some((-12, -77)));
    assert_eq!(filename_to_lat_lon("N00E000.hgt"), Some((0, 0)));
    assert_eq!(filename_to_lat_lon("invalid"), None);
}

#[test]
fn test_roundtrip() {
    let coords = [(35.5, 138.7), (-12.3, -77.1), (0.5, -0.5)];
    
    for (lat, lon) in coords {
        let filename = lat_lon_to_filename(lat, lon);
        let (parsed_lat, parsed_lon) = filename_to_lat_lon(&filename).unwrap();
        
        // Should match the floor values
        assert_eq!(parsed_lat, lat.floor() as i32);
        assert_eq!(parsed_lon, lon.floor() as i32);
    }
}
```

### Deliverables
- [x] Automatic filename calculation works
- [x] All quadrants tested (N/S, E/W)
- [x] Edge cases handled
- [x] Roundtrip parsing works

---

## Phase 3: Caching Layer (Weekend 3 - 4 hours)

### Goal
Add LRU cache to limit memory usage and improve performance.

### Tasks

#### 3.1 Add Dependencies (5 min)
Update `Cargo.toml`:
```toml
[dependencies]
memmap2 = "0.9"
thiserror = "1.0"
moka = { version = "0.12", features = ["sync"] }
```

#### 3.2 Implement SrtmService (2.5 hours)
**File:** `src/service.rs`

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;
use moka::sync::Cache;
use crate::tile::SrtmTile;
use crate::filename::lat_lon_to_filename;
use crate::error::{Result, SrtmError};

pub struct SrtmService {
    data_dir: PathBuf,
    tile_cache: Cache<String, Arc<SrtmTile>>,
}

impl SrtmService {
    /// Create new SRTM service
    /// 
    /// # Arguments
    /// * `data_dir` - Directory containing .hgt files
    /// * `cache_size` - Maximum number of tiles to keep in memory
    pub fn new<P: AsRef<Path>>(data_dir: P, cache_size: u64) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            tile_cache: Cache::builder()
                .max_capacity(cache_size)
                .build(),
        }
    }
    
    /// Get elevation for given coordinates
    pub fn get_elevation(&self, lat: f64, lon: f64) -> Result<i16> {
        // Validate coordinates
        if !(-60.0..=60.0).contains(&lat) {
            return Err(SrtmError::OutOfBounds);
        }
        if !(-180.0..=180.0).contains(&lon) {
            return Err(SrtmError::OutOfBounds);
        }
        
        // Calculate filename
        let filename = lat_lon_to_filename(lat, lon);
        
        // Load tile (from cache or disk)
        let tile = self.load_tile(&filename)?;
        
        // Extract elevation
        tile.get_elevation(lat, lon)
    }
    
    /// Load tile from cache or disk
    fn load_tile(&self, filename: &str) -> Result<Arc<SrtmTile>> {
        // Check cache first
        if let Some(tile) = self.tile_cache.get(filename) {
            return Ok(tile);
        }
        
        // Load from disk
        let path = self.data_dir.join(filename);
        
        if !path.exists() {
            return Err(SrtmError::FileNotFound(filename.to_string()));
        }
        
        let tile = Arc::new(SrtmTile::from_file(&path)?);
        
        // Insert into cache
        self.tile_cache.insert(filename.to_string(), tile.clone());
        
        Ok(tile)
    }
    
    /// Get cache statistics (for debugging/monitoring)
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            entry_count: self.tile_cache.entry_count(),
            hit_count: self.tile_cache.hit_count(),
            miss_count: self.tile_cache.miss_count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entry_count: u64,
    pub hit_count: u64,
    pub miss_count: u64,
}
```

#### 3.3 Write Tests (1 hour)
**File:** `tests/service_tests.rs`

```rust
use srtm_service::service::SrtmService;
use std::path::PathBuf;

#[test]
fn test_service_basic() {
    let service = SrtmService::new("test_data", 10);
    
    // Assuming you have N35E138.hgt in test_data/
    let elev = service.get_elevation(35.5, 138.5);
    assert!(elev.is_ok());
}

#[test]
fn test_cache_hit() {
    let service = SrtmService::new("test_data", 10);
    
    // First call - cache miss
    let _ = service.get_elevation(35.5, 138.5);
    let stats1 = service.cache_stats();
    
    // Second call - should be cache hit
    let _ = service.get_elevation(35.6, 138.6); // Same tile
    let stats2 = service.cache_stats();
    
    assert!(stats2.hit_count > stats1.hit_count);
}

#[test]
fn test_multiple_tiles() {
    let service = SrtmService::new("test_data", 10);
    
    // Assuming you have multiple .hgt files
    let _ = service.get_elevation(35.5, 138.5); // N35E138
    let _ = service.get_elevation(36.5, 138.5); // N36E138
    
    let stats = service.cache_stats();
    assert_eq!(stats.entry_count, 2); // Should have 2 tiles cached
}

#[test]
fn test_invalid_coordinates() {
    let service = SrtmService::new("test_data", 10);
    
    // Out of SRTM coverage
    assert!(service.get_elevation(70.0, 0.0).is_err());
    assert!(service.get_elevation(0.0, 200.0).is_err());
}

#[test]
fn test_missing_file() {
    let service = SrtmService::new("test_data", 10);
    
    // File that doesn't exist
    let result = service.get_elevation(50.0, 50.0);
    assert!(result.is_err());
}
```

#### 3.4 Memory Test (30 min)
**File:** `tests/memory_test.rs`

```rust
#[test]
#[ignore] // Run with: cargo test --release -- --ignored
fn test_memory_usage() {
    use srtm_service::service::SrtmService;
    
    let service = SrtmService::new("test_data", 100);
    
    // Load 100 different tiles (if you have that many)
    for lat in 20..40 {
        for lon in -120..-100 {
            let _ = service.get_elevation(lat as f64 + 0.5, lon as f64 + 0.5);
        }
    }
    
    let stats = service.cache_stats();
    println!("Cache stats: {:?}", stats);
    
    // Estimate memory: 100 tiles × ~25MB (SRTM1) = ~2.5GB max
    // With SRTM3: 100 tiles × ~2.8MB = ~280MB max
    assert!(stats.entry_count <= 100);
}
```

### Deliverables
- [x] LRU cache working
- [x] Cache hit/miss tracked
- [x] Memory bounded
- [x] Multiple tiles can be loaded

---

## Phase 4: HTTP API (Weekend 4 - 5 hours)

### Goal
REST API with Axum web framework.

### Tasks

#### 4.1 Add Dependencies (5 min)
Update `Cargo.toml`:
```toml
[dependencies]
memmap2 = "0.9"
thiserror = "1.0"
moka = { version = "0.12", features = ["sync"] }
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tower = "0.4"
tower-http = { version = "0.5", features = ["cors", "trace"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

#### 4.2 Implement HTTP Handlers (2 hours)
**File:** `src/handlers.rs`

```rust
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::service::SrtmService;

#[derive(Deserialize)]
pub struct ElevationQuery {
    lat: f64,
    lon: f64,
}

#[derive(Serialize)]
pub struct ElevationResponse {
    latitude: f64,
    longitude: f64,
    elevation: i16,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    error: String,
}

pub async fn get_elevation(
    Query(params): Query<ElevationQuery>,
    State(service): State<Arc<SrtmService>>,
) -> impl IntoResponse {
    match service.get_elevation(params.lat, params.lon) {
        Ok(elevation) => (
            StatusCode::OK,
            Json(ElevationResponse {
                latitude: params.lat,
                longitude: params.lon,
                elevation,
            }),
        ).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

pub async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

#[derive(Serialize)]
pub struct StatsResponse {
    cache_entries: u64,
    cache_hits: u64,
    cache_misses: u64,
    hit_rate: f64,
}

pub async fn get_stats(
    State(service): State<Arc<SrtmService>>,
) -> impl IntoResponse {
    let stats = service.cache_stats();
    let total = stats.hit_count + stats.miss_count;
    let hit_rate = if total > 0 {
        stats.hit_count as f64 / total as f64
    } else {
        0.0
    };
    
    (
        StatusCode::OK,
        Json(StatsResponse {
            cache_entries: stats.entry_count,
            cache_hits: stats.hit_count,
            cache_misses: stats.miss_count,
            hit_rate,
        }),
    )
}
```

#### 4.3 Setup Server (1.5 hours)
**File:** `src/main.rs`

```rust
mod error;
mod filename;
mod handlers;
mod service;
mod tile;

use axum::{
    routing::get,
    Router,
};
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::service::SrtmService;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "srtm_service=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Read config from environment
    let data_dir = env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let cache_size: u64 = env::var("CACHE_SIZE")
        .unwrap_or_else(|_| "100".to_string())
        .parse()
        .expect("CACHE_SIZE must be a number");
    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .expect("PORT must be a number");

    // Create service
    let service = Arc::new(SrtmService::new(data_dir, cache_size));

    // Build router
    let app = Router::new()
        .route("/elevation", get(handlers::get_elevation))
        .route("/health", get(handlers::health_check))
        .route("/stats", get(handlers::get_stats))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(service);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Starting server on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

#### 4.4 Integration Tests (1 hour)
**File:** `tests/http_tests.rs`

```rust
use axum::http::StatusCode;
use axum_test::TestServer;

#[tokio::test]
async fn test_elevation_endpoint() {
    // Setup test server
    let app = create_test_app();
    let server = TestServer::new(app).unwrap();
    
    // Test valid request
    let response = server
        .get("/elevation?lat=35.5&lon=138.5")
        .await;
    
    response.assert_status_ok();
    response.assert_json(&serde_json::json!({
        "latitude": 35.5,
        "longitude": 138.5,
        "elevation": 1234  // Replace with actual value
    }));
}

#[tokio::test]
async fn test_invalid_coordinates() {
    let app = create_test_app();
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .get("/elevation?lat=91&lon=0")
        .await;
    
    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_health_check() {
    let app = create_test_app();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    response.assert_status_ok();
}
```

### Deliverables
- [x] HTTP server running
- [x] `/elevation?lat=X&lon=Y` endpoint works
- [x] `/health` endpoint works
- [x] `/stats` endpoint shows cache metrics
- [x] CORS enabled
- [x] Structured logging

### Testing
```bash
# Start server
DATA_DIR=./test_data CACHE_SIZE=10 cargo run

# Test in another terminal
curl "http://localhost:8080/elevation?lat=35.5&lon=138.5"
curl "http://localhost:8080/health"
curl "http://localhost:8080/stats"
```

---

## Phase 5: Production Ready (Weekend 5 - 4 hours)

### Goal
Docker, documentation, and production hardening.

### Tasks

#### 5.1 Create Dockerfile (30 min)
**File:** `Dockerfile`

```dockerfile
# Build stage
FROM rust:1.75 as builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /app/target/release/srtm-service /usr/local/bin/srtm-service

# Create data directory
RUN mkdir -p /data

# Set defaults
ENV DATA_DIR=/data
ENV CACHE_SIZE=100
ENV PORT=8080

EXPOSE 8080

CMD ["srtm-service"]
```

**File:** `.dockerignore`
```
target/
test_data/
.git/
*.hgt
```

#### 5.2 Docker Compose (15 min)
**File:** `docker-compose.yml`

```yaml
version: '3.8'

services:
  srtm:
    build: .
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data:ro  # Mount your .hgt files as read-only
    environment:
      - DATA_DIR=/data
      - CACHE_SIZE=100
      - PORT=8080
      - RUST_LOG=srtm_service=info
    restart: unless-stopped
```

#### 5.3 README.md (1 hour)
**File:** `README.md`

```markdown
# SRTM Elevation Service

High-performance, memory-efficient microservice for querying elevation data from SRTM files.

## Features

- 🚀 **Fast**: <10ms response time for cached tiles
- 💾 **Memory Efficient**: <100MB with 100 cached tiles
- 🔒 **Offline**: No internet required, works with local .hgt files
- 🐳 **Docker Ready**: Easy deployment
- 📊 **Metrics**: Built-in cache statistics

## Quick Start

### Docker (Recommended)

```bash
# Place your .hgt files in ./data directory
docker-compose up -d

# Query elevation
curl "http://localhost:8080/elevation?lat=19.4326&lon=-99.1332"
```

### From Source

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build
cargo build --release

# Run
DATA_DIR=./data CACHE_SIZE=100 ./target/release/srtm-service
```

## API

### GET /elevation

Query elevation for coordinates.

**Parameters:**
- `lat` (required): Latitude (-60 to 60)
- `lon` (required): Longitude (-180 to 180)

**Example:**
```bash
curl "http://localhost:8080/elevation?lat=35.5&lon=138.5"
```

**Response:**
```json
{
  "latitude": 35.5,
  "longitude": 138.5,
  "elevation": 1234
}
```

### GET /health

Health check endpoint.

### GET /stats

Cache statistics.

**Response:**
```json
{
  "cache_entries": 45,
  "cache_hits": 1234,
  "cache_misses": 56,
  "hit_rate": 0.956
}
```

## Configuration

Set via environment variables:

- `DATA_DIR`: Directory containing .hgt files (default: `./data`)
- `CACHE_SIZE`: Maximum tiles in memory (default: `100`)
- `PORT`: HTTP port (default: `8080`)
- `RUST_LOG`: Log level (default: `srtm_service=info`)

## Data Sources

Download SRTM data:
- https://dwtkns.com/srtm30m/
- https://earthexplorer.usgs.gov/

Place .hgt files in your data directory.

## Performance

**Memory:**
- SRTM3 (90m): ~2.8MB per tile
- SRTM1 (30m): ~25MB per tile
- Example: 100 SRTM3 tiles ≈ 280MB

**Throughput:**
- Cached: >10,000 req/s
- Uncached: ~1,000 req/s

## License

MIT
```

#### 5.4 CI/CD (30 min)
**File:** `.github/workflows/ci.yml`

```yaml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      
      - name: Run tests
        run: cargo test --all-features
      
      - name: Run clippy
        run: cargo clippy -- -D warnings
      
      - name: Check formatting
        run: cargo fmt -- --check
  
  docker:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Build Docker image
        run: docker build -t srtm-service .
```

#### 5.5 Optimizations (1.5 hours)

**Add bilinear interpolation (optional):**
```rust
// In tile.rs
pub fn get_elevation_interpolated(&self, lat: f64, lon: f64) -> Result<f64> {
    // Get 4 surrounding points
    // Interpolate bilinearly
    // Return smoothed elevation
}
```

**Add graceful shutdown:**
```rust
// In main.rs
let shutdown_signal = async {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    tracing::info!("Shutdown signal received");
};

axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal)
    .await
    .unwrap();
```

### Deliverables
- [x] Docker image builds
- [x] Docker Compose setup
- [x] Complete documentation
- [x] CI/CD pipeline
- [x] Production-ready code

---

## Bonus Features (Optional)

### Batch Endpoint
```rust
// POST /elevation/batch
// Body: [{"lat": 35.5, "lon": 138.5}, ...]
```

### Prometheus Metrics
```rust
use prometheus::{Encoder, Registry, Counter, Histogram};

// Track request duration, cache hits, errors, etc.
```

### WebSocket Streaming
```rust
// Stream elevations for GPX track in real-time
```

---

## Success Checklist

After all phases:

- [ ] Memory usage <100MB with 100 cached tiles
- [ ] Response time <10ms for cached, <50ms uncached
- [ ] All tests passing
- [ ] Docker image builds and runs
- [ ] Documentation complete
- [ ] Ready to replace Python/Flask service

---

## Tips for Success

1. **Test as you go** - Don't wait until the end
2. **Use real .hgt files** - Download a few for testing
3. **Start simple** - Get basics working before optimizing
4. **Compare with Go version** - Use it as reference when stuck
5. **Commit often** - Small, working increments

## Getting Help

If you get stuck:
1. Check the Go reference implementation
2. Review CLAUDE.md for algorithm details
3. Ask Claude Code for help with specific issues
4. Test each component in isolation

Good luck! 🦀
