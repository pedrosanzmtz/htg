# SRTM Elevation Microservice (Rust)

## Project Overview

Building a high-performance, memory-efficient microservice to return elevation data from SRTM (Shuttle Radar Topography Mission) .hgt files given latitude/longitude coordinates.

**Current Problem:** Existing Python/Flask service uses 7GB of memory
**Goal:** Rust microservice using <100MB with same functionality

## Current Status

**Phase:** All core phases complete (1-5)
**Latest Features:** Bilinear interpolation, GeoJSON batch queries, auto-download, ArduPilot source support

### Open Issues
- #6: Publish htg library to crates.io
- #7: Publish htg-service to DockerHub
- #18: Multi-architecture Docker builds (ARM64/ARM)
- #21: Performance benchmarks for memory and latency
- #22: Compare elevation results with popular APIs

## Technical Context

### SRTM Data Format
- **File Format:** Binary .hgt files with 16-bit big-endian signed integers
- **Filename Convention:** `N35E138.hgt` (latitude N35°, longitude E138°)
  - North/South prefix for latitude (N for ≥0, S for <0)
  - East/West prefix for longitude (E for ≥0, W for <0)
  - Always 2 digits for lat, 3 digits for lon
- **Resolutions:**
  - SRTM1: 3601×3601 samples (1 arc-second, ~30m) = 25,934,402 bytes
  - SRTM3: 1201×1201 samples (3 arc-second, ~90m) = 2,884,802 bytes
- **Coverage:** ±60° latitude globally
- **Data Layout:** Row-major order, top-to-bottom (north-to-south), left-to-right (west-to-east)
- **Void Value:** -32768 indicates no data (ocean, missing coverage)

### Key Features
1. **Offline/On-Premises:** All .hgt files provided locally
2. **Auto-Download:** Optional automatic download of missing tiles (feature flag)
3. **Automatic File Detection:** Given (lat, lon) → determine which .hgt file to open
4. **Memory Efficient:** LRU cache to limit memory usage
5. **High Performance:** Memory-mapped I/O for fast access
6. **Bilinear Interpolation:** Sub-pixel accuracy for elevation queries
7. **GeoJSON Batch Queries:** Process multiple coordinates in one request
8. **REST API:** HTTP endpoints for elevation queries

### Reference Implementation
The Go library `asmyasnikov/srtm` was used as algorithm reference:
- https://github.com/asmyasnikov/srtm

## Architecture

```
HTTP Request (lat, lon)
    ↓
Axum Web Service
    ↓
Filename Calculator: (lat, lon) → "N35E138.hgt"
    ↓
Tile Cache (LRU): Check if tile already loaded
    ↓                           ↓ (cache miss + auto-download enabled)
Tile Loader: Memory-map        Download from configured URL
    ↓
Elevation Extractor: Nearest-neighbor or bilinear interpolation
    ↓
Return elevation value (i16 or f64)
```

## Tech Stack

### Core Dependencies (htg library)
- `memmap2` - Memory-mapped file I/O
- `moka` - LRU cache with async support
- `thiserror` - Error handling

### Optional Dependencies (htg library)
- `reqwest` - HTTP client for auto-download (feature: `download`)
- `flate2` - Gzip decompression (feature: `download`)

### HTTP Service Dependencies (htg-service)
- `axum` - Web framework
- `tokio` - Async runtime
- `serde` / `serde_json` - JSON serialization
- `geojson` - GeoJSON parsing for batch queries
- `tower` / `tower-http` - Middleware (CORS, tracing)
- `tracing` / `tracing-subscriber` - Structured logging

## Project Structure

This is a **Cargo workspace** with two crates:

```
htg/                            # Workspace root
├── Cargo.toml                  # Workspace manifest
├── Cargo.lock                  # Locked dependencies
├── CLAUDE.md                   # This file (AI context)
├── README.md                   # Project documentation
├── ROADMAP.md                  # Implementation roadmap
├── CONTRIBUTING.md             # Contribution guidelines
├── Dockerfile                  # Multi-stage Docker build
├── docker-compose.yml          # Docker Compose config
├── .dockerignore               # Docker ignore patterns
├── .gitignore                  # Git ignore patterns
├── .github/
│   └── workflows/
│       └── ci.yml              # GitHub Actions CI/CD
├── htg/                        # Library crate (publish to crates.io)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # Library entry point, re-exports
│       ├── tile.rs             # SrtmTile struct, elevation extraction
│       ├── service.rs          # SrtmService with caching
│       ├── filename.rs         # Lat/lon ↔ filename conversion
│       ├── download.rs         # Auto-download feature (optional)
│       └── error.rs            # Custom error types (SrtmError)
└── htg-service/                # Binary crate (publish to DockerHub)
    ├── Cargo.toml
    ├── src/
    │   ├── main.rs             # Entry point, Axum setup
    │   └── handlers.rs         # HTTP handlers (GET/POST elevation)
    └── tests/
        └── api_tests.rs        # Integration tests
```

### Publishing Targets
- **htg** library → crates.io
- **htg-service** binary → DockerHub

## API Endpoints

### GET /elevation
Query elevation for a single coordinate.

```bash
# Nearest-neighbor lookup
curl "http://localhost:8080/elevation?lat=35.3606&lon=138.7274"
# {"elevation": 3776, "lat": 35.3606, "lon": 138.7274}

# Bilinear interpolation
curl "http://localhost:8080/elevation?lat=35.3606&lon=138.7274&interpolate=true"
# {"elevation": 3776.42, "lat": 35.3606, "lon": 138.7274, "interpolated": true}
```

### POST /elevation
Batch elevation query with GeoJSON geometry.

```bash
curl -X POST "http://localhost:8080/elevation" \
  -H "Content-Type: application/json" \
  -d '{"type": "LineString", "coordinates": [[138.5, 35.5], [139.0, 35.0]]}'
# {"type": "LineString", "coordinates": [[138.5, 35.5, 500], [139.0, 35.0, 420]]}
```

Supported geometry types: Point, MultiPoint, LineString, MultiLineString, Polygon, MultiPolygon, GeometryCollection.

### GET /health
Health check endpoint.

```bash
curl "http://localhost:8080/health"
# {"status": "healthy", "version": "0.1.0"}
```

### GET /stats
Cache statistics.

```bash
curl "http://localhost:8080/stats"
# {"cached_tiles": 5, "cache_hits": 150, "cache_misses": 5, "hit_rate": 0.967}
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `HTG_DATA_DIR` | Directory containing .hgt files | Required |
| `HTG_CACHE_SIZE` | Maximum tiles in LRU cache | 100 |
| `HTG_PORT` | HTTP server port | 8080 |
| `HTG_DOWNLOAD_SOURCE` | Named source: "ardupilot", "ardupilot-srtm1", "ardupilot-srtm3" | None |
| `HTG_DOWNLOAD_URL` | URL template for auto-download (use `{filename}`, `{continent}` placeholders) | None |
| `HTG_DOWNLOAD_GZIP` | Whether downloads are gzipped | false |
| `RUST_LOG` | Log level (e.g., "info", "debug", "htg_service=debug") | "info" |

## Development Phases

### Phase 1: Core Tile Parser ✓
- [x] `SrtmTile` struct with `data: Mmap` and `samples: usize`
- [x] `from_file(path: &Path)` - memory-map file, detect SRTM1 vs SRTM3
- [x] `get_elevation(lat: f64, lon: f64)` - calculate row/col, read bytes
- [x] `get_elevation_interpolated()` - bilinear interpolation
- [x] Unit tests

### Phase 2: Filename Detection ✓
- [x] `lat_lon_to_filename(lat: f64, lon: f64) -> String`
- [x] `filename_to_lat_lon(filename: &str) -> Option<(i32, i32)>`
- [x] Handle edge cases: negative coords, dateline, poles
- [x] Comprehensive tests

### Phase 3: Caching Layer ✓
- [x] `SrtmService` struct with `Cache<String, Arc<SrtmTile>>`
- [x] `get_elevation(lat: f64, lon: f64)` - cache hit/miss logic
- [x] `get_elevation_interpolated()` - interpolation support
- [x] Configurable cache size
- [x] Cache statistics (hits, misses, hit rate)
- [x] Auto-download missing tiles (optional feature)

### Phase 4: HTTP API ✓
- [x] Axum router with GET `/elevation` endpoint
- [x] POST `/elevation` for GeoJSON batch queries
- [x] Query params: `?lat=X&lon=Y&interpolate=true`
- [x] JSON responses with proper error handling
- [x] CORS support
- [x] Health check endpoint `/health`
- [x] Statistics endpoint `/stats`

### Phase 5: Production Readiness ✓
- [x] Configuration via environment variables
- [x] Structured logging with `tracing`
- [x] Multi-stage Dockerfile
- [x] Docker Compose support
- [x] GitHub Actions CI/CD (format, clippy, test, build, docker)
- [x] Comprehensive error handling
- [x] README documentation

## Algorithm Details

### 1. Filename Calculation
```rust
fn lat_lon_to_filename(lat: f64, lon: f64) -> String {
    let lat_int = lat.floor() as i32;
    let lon_int = lon.floor() as i32;

    let lat_prefix = if lat_int >= 0 { "N" } else { "S" };
    let lon_prefix = if lon_int >= 0 { "E" } else { "W" };

    format!(
        "{}{:02}{}{:03}.hgt",
        lat_prefix, lat_int.abs(),
        lon_prefix, lon_int.abs()
    )
}

// Examples:
// (35.5, 138.7) → "N35E138.hgt"
// (-12.3, -77.1) → "S13W078.hgt"
```

### 2. Elevation Extraction (Nearest-Neighbor)
```rust
fn get_elevation(&self, lat: f64, lon: f64) -> Result<i16> {
    let lat_frac = lat - lat.floor();
    let lon_frac = lon - lon.floor();

    // Rows are inverted (north to south)
    let row = ((1.0 - lat_frac) * (self.samples - 1) as f64).round() as usize;
    let col = (lon_frac * (self.samples - 1) as f64).round() as usize;

    let offset = (row * self.samples + col) * 2;
    Ok(i16::from_be_bytes([self.data[offset], self.data[offset + 1]]))
}
```

### 3. Bilinear Interpolation
```rust
fn get_elevation_interpolated(&self, lat: f64, lon: f64) -> Result<Option<f64>> {
    // Get 4 surrounding grid points
    let v00 = self.get_elevation_at(row0, col0)?;
    let v10 = self.get_elevation_at(row0, col1)?;
    let v01 = self.get_elevation_at(row1, col0)?;
    let v11 = self.get_elevation_at(row1, col1)?;

    // Return None if any point is void (-32768)
    if [v00, v10, v01, v11].iter().any(|&v| v == VOID_VALUE) {
        return Ok(None);
    }

    // Bilinear interpolation
    let v0 = v00 + (v10 - v00) * col_weight;
    let v1 = v01 + (v11 - v01) * col_weight;
    Ok(Some(v0 + (v1 - v0) * row_weight))
}
```

### 4. Cache Strategy
- Use `moka::sync::Cache` with configurable max capacity
- Key: filename (String)
- Value: `Arc<SrtmTile>` (shared ownership for concurrent access)
- Eviction: LRU (least recently used)

## Testing

### Running Tests
```bash
# All tests
cargo test

# With output
cargo test -- --nocapture

# Specific crate
cargo test -p htg
cargo test -p htg-service
```

### Test Coverage
- **htg library:** 35 unit tests
- **htg-service:** 14 integration tests
- **Doc tests:** 2 tests

### CI/CD Pipeline
GitHub Actions runs on every push/PR:
1. Format check (`cargo fmt --check`)
2. Clippy lints (`cargo clippy`)
3. Tests (`cargo test`)
4. Build (`cargo build --release`)
5. Docker build

## Docker

### Building
```bash
docker build -t htg-service .
```

### Running
```bash
docker run -p 8080:8080 \
  -v /path/to/srtm:/data/srtm:ro \
  -e HTG_DATA_DIR=/data/srtm \
  htg-service
```

### Docker Compose
```bash
docker-compose up
```

## Example Usage

```bash
# Start service with ArduPilot auto-download (recommended)
export HTG_DATA_DIR=/path/to/hgt/files
export HTG_DOWNLOAD_SOURCE=ardupilot
cargo run --release -p htg-service

# Or start without auto-download (local files only)
export HTG_DATA_DIR=/path/to/hgt/files
export HTG_CACHE_SIZE=100
cargo run --release -p htg-service

# Query elevation (Mount Fuji)
curl "http://localhost:8080/elevation?lat=35.3606&lon=138.7274"
# {"elevation": 3776, "lat": 35.3606, "lon": 138.7274}

# Query with interpolation
curl "http://localhost:8080/elevation?lat=35.3606&lon=138.7274&interpolate=true"
# {"elevation": 3776.42, "lat": 35.3606, "lon": 138.7274, "interpolated": true}

# Batch query (elevation profile)
curl -X POST "http://localhost:8080/elevation" \
  -H "Content-Type: application/json" \
  -d '{"type": "LineString", "coordinates": [[138.5, 35.5], [138.6, 35.6], [138.7, 35.7]]}'

# Health check
curl "http://localhost:8080/health"

# Cache stats
curl "http://localhost:8080/stats"
```

## Success Criteria

- [x] Memory usage <100MB with 100 cached tiles
- [x] Response time <10ms for cached tiles
- [x] Response time <50ms for uncached tiles
- [x] Handles 1000+ requests/second
- [x] Graceful error handling
- [x] Production-ready Docker image
- [x] Bilinear interpolation support
- [x] GeoJSON batch queries

## Common Pitfalls to Avoid

1. **Row Inversion:** Rows go from north to south, so row 0 = top = north edge
2. **File Size Detection:** Don't hardcode - detect SRTM1 vs SRTM3 from file size
3. **Coordinate Edge Cases:**
   - Coordinates exactly on tile boundary
   - Negative zero handling
   - Out-of-bounds coordinates (±60° lat, ±180° lon)
4. **Memory Safety:** Use `Arc<SrtmTile>` for shared cache access
5. **Error Handling:** Gracefully handle missing .hgt files
6. **Void Values:** -32768 indicates no data, handle in interpolation
7. **GeoJSON Coordinate Order:** GeoJSON uses [lon, lat], not [lat, lon]

## Resources

- SRTM Data Download: https://dwtkns.com/srtm30m/
- Reference Implementation: https://github.com/asmyasnikov/srtm
- HGT Format Spec: https://dds.cr.usgs.gov/srtm/version2_1/Documentation/
- Axum Docs: https://docs.rs/axum/latest/axum/
- Moka Cache: https://docs.rs/moka/latest/moka/
- GeoJSON Spec: https://geojson.org/

## Notes for Claude Code

- All .hgt files should be in a local directory (configured via `HTG_DATA_DIR`)
- Target deployment: Docker container
- Use proper error types, not unwrap/panic in production code
- Run `cargo fmt` and `cargo clippy` before committing
- All tests must pass before merging PRs
