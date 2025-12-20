# SRTM Elevation Microservice (Rust)

## Project Overview
Building a high-performance, memory-efficient microservice to return elevation data from SRTM (Shuttle Radar Topography Mission) .hgt files given latitude/longitude coordinates.

**Current Problem:** Existing Python/Flask service uses 7GB of memory  
**Goal:** Rust microservice using <100MB with same functionality

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

### Key Requirements
1. **Offline/On-Premises:** No internet access, all .hgt files provided locally
2. **Automatic File Detection:** Given (lat, lon) → determine which .hgt file to open
3. **Memory Efficient:** LRU cache to limit memory usage
4. **High Performance:** Memory-mapped I/O for fast access
5. **REST API:** HTTP endpoint `/elevation?lat=X&lon=Y`

### Reference Implementation
The Go library `asmyasnikov/srtm` already solves this problem:
- https://github.com/asmyasnikov/srtm
- Study it for algorithm reference, but implement from scratch in Rust

## Architecture

```
HTTP Request (lat, lon)
    ↓
Axum Web Service
    ↓
Filename Calculator: (lat, lon) → "N35E138.hgt"
    ↓
Tile Cache (LRU): Check if tile already loaded
    ↓
Tile Loader: Memory-map .hgt file
    ↓
Elevation Extractor: Calculate row/col, read 2 bytes
    ↓
Return elevation value
```

## Tech Stack

### Core Dependencies
- `axum` - Web framework
- `tokio` - Async runtime
- `memmap2` - Memory-mapped file I/O
- `moka` - LRU cache
- `serde` - JSON serialization
- `tower` - Middleware (CORS, logging, etc.)

### Optional/Nice-to-Have
- `tracing` - Structured logging
- `prometheus` - Metrics
- `clap` - CLI argument parsing

## Project Structure

This is a **Cargo workspace** with two crates:

```
htg/                        # Workspace root
├── Cargo.toml              # Workspace manifest
├── CLAUDE.md               # This file
├── ROADMAP.md              # Implementation roadmap
├── README.md               # Project documentation
├── htg/                    # Library crate (publish to crates.io)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs          # Library entry point
│   │   ├── tile.rs         # SrtmTile struct, elevation extraction
│   │   ├── service.rs      # SrtmService with caching
│   │   ├── filename.rs     # Lat/lon → filename conversion
│   │   └── error.rs        # Custom error types
│   └── tests/
│       ├── tile_tests.rs
│       └── filename_tests.rs
├── htg-service/            # Binary crate (publish to DockerHub)
│   ├── Cargo.toml
│   ├── Dockerfile
│   ├── src/
│   │   ├── main.rs         # Entry point, Axum setup
│   │   └── handlers.rs     # HTTP handlers
│   └── tests/
│       └── integration_tests.rs
└── .hgt files/             # Local SRTM data (not in repo)
```

### Publishing Targets
- **htg** library → crates.io
- **htg-service** binary → DockerHub

## Development Phases

### Phase 1: Core Tile Parser ✓
**Goal:** Read a single .hgt file and extract elevation
- [ ] `SrtmTile` struct with `data: Mmap` and `samples: usize`
- [ ] `from_file(path: &Path)` - memory-map file, detect SRTM1 vs SRTM3
- [ ] `get_elevation(lat: f64, lon: f64)` - calculate row/col, read bytes
- [ ] Unit tests with sample .hgt file

### Phase 2: Filename Detection ✓
**Goal:** Automatically determine which .hgt file for any coordinate
- [ ] `lat_lon_to_filename(lat: f64, lon: f64) -> String`
- [ ] Handle edge cases: negative coords, -0.5, dateline, poles
- [ ] Comprehensive tests

### Phase 3: Caching Layer ✓
**Goal:** LRU cache to limit memory usage
- [ ] `SrtmService` struct with `Cache<String, Arc<SrtmTile>>`
- [ ] `get_elevation(lat: f64, lon: f64)` - cache hit/miss logic
- [ ] Configurable cache size
- [ ] Test memory bounds

### Phase 4: HTTP API ✓
**Goal:** REST endpoint for elevation queries
- [ ] Axum router with `/elevation` endpoint
- [ ] Query params: `?lat=X&lon=Y`
- [ ] JSON response: `{"elevation": 1234}` or `{"error": "..."}`
- [ ] CORS support
- [ ] Health check endpoint `/health`

### Phase 5: Production Readiness ✓
**Goal:** Deploy-ready microservice
- [ ] Configuration via env vars or config file
- [ ] Structured logging with `tracing`
- [ ] Docker support
- [ ] Metrics (optional)
- [ ] Error handling and validation
- [ ] Documentation

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
        lat_prefix,
        lat_int.abs(),
        lon_prefix,
        lon_int.abs()
    )
}

// Examples:
// (35.5, 138.7) → "N35E138.hgt"
// (-12.3, -77.1) → "S12W077.hgt"
```

### 2. Elevation Extraction
```rust
fn get_elevation(&self, lat: f64, lon: f64) -> Result<i16, Error> {
    // 1. Get fractional position within tile
    let lat_int = lat.floor();
    let lon_int = lon.floor();
    let lat_frac = lat - lat_int;
    let lon_frac = lon - lon_int;
    
    // 2. Convert to row/col (IMPORTANT: rows are inverted!)
    let row = ((1.0 - lat_frac) * (self.samples - 1) as f64) as usize;
    let col = (lon_frac * (self.samples - 1) as f64) as usize;
    
    // 3. Calculate byte offset (2 bytes per sample)
    let offset = (row * self.samples + col) * 2;
    
    // 4. Read 16-bit big-endian value
    Ok(i16::from_be_bytes([
        self.data[offset],
        self.data[offset + 1],
    ]))
}
```

### 3. Cache Strategy
- Use `moka::sync::Cache` with configurable max capacity
- Key: filename (String)
- Value: `Arc<SrtmTile>` (shared ownership for concurrent access)
- Eviction: LRU (least recently used)

## Testing Strategy

### Unit Tests
- Filename generation for all quadrants (N/S, E/W)
- Edge cases: equator, prime meridian, dateline
- Elevation extraction accuracy

### Integration Tests
- HTTP endpoint with real .hgt files
- Cache hit/miss behavior
- Error handling (missing files, invalid coords)

### Performance Tests
- Memory usage under load
- Request throughput
- Cache efficiency

## Common Pitfalls to Avoid

1. **Row Inversion:** Rows go from north to south, so row 0 = top = north edge
2. **File Size Detection:** Don't hardcode - detect SRTM1 vs SRTM3 from file size
3. **Coordinate Edge Cases:** 
   - Coordinates exactly on tile boundary
   - Negative zero handling
   - Out-of-bounds coordinates
4. **Memory Safety:** Use `Arc<SrtmTile>` for shared cache access
5. **Error Handling:** Gracefully handle missing .hgt files

## Example Usage

```bash
# Start service
export DATA_DIR=/path/to/hgt/files
export CACHE_SIZE=100
cargo run --release

# Query elevation
curl "http://localhost:8080/elevation?lat=19.4326&lon=-99.1332"
# {"elevation": 2240}

# Invalid coordinates
curl "http://localhost:8080/elevation?lat=91&lon=0"
# {"error": "Latitude out of range"}
```

## Success Criteria

- ✅ Memory usage <100MB with 100 cached tiles
- ✅ Response time <10ms for cached tiles
- ✅ Response time <50ms for uncached tiles
- ✅ Handles 1000+ requests/second
- ✅ Graceful error handling
- ✅ Production-ready Docker image

## Resources

- SRTM Data Download: https://dwtkns.com/srtm30m/
- Reference Implementation: https://github.com/asmyasnikov/srtm
- HGT Format Spec: https://dds.cr.usgs.gov/srtm/version2_1/Documentation/
- Axum Docs: https://docs.rs/axum/latest/axum/
- Moka Cache: https://docs.rs/moka/latest/moka/

---

## Current Status

**Phase:** Not started  
**Next Steps:** Initialize Rust project, add dependencies, implement Phase 1

## Notes for Claude Code

- All .hgt files are in `/data/srtm/` (local, offline)
- Target deployment: Docker container
- Prioritize correctness over optimization initially
- Add tests as you go, not at the end
- Use proper error types, not unwrap/panic in production code
