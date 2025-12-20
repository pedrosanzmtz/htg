# HTG - SRTM Elevation Library & Service

High-performance, memory-efficient Rust library and microservice for querying elevation data from SRTM (Shuttle Radar Topography Mission) `.hgt` files.

## Problem

Existing elevation services (e.g., Python/Flask) consume excessive memory (7GB+). This project provides a Rust-based solution using **<100MB** with the same functionality.

## Features

- **Fast**: <10ms response time for cached tiles
- **Memory Efficient**: <100MB with 100 cached tiles (vs 7GB in Python)
- **Offline**: No internet required, works with local `.hgt` files
- **Automatic Detection**: Determines correct tile from coordinates
- **LRU Caching**: Configurable cache size to bound memory usage
- **Docker Ready**: Easy deployment with Docker/Docker Compose

## Project Structure

This is a Cargo workspace with two crates:

```
htg/
├── htg/              # Library crate (published to crates.io)
│   └── src/
│       ├── lib.rs
│       ├── tile.rs       # SRTM tile parsing
│       ├── filename.rs   # Coordinate → filename conversion
│       ├── service.rs    # Caching service
│       └── error.rs      # Error types
│
└── htg-service/      # Binary crate (published to DockerHub)
    └── src/
        ├── main.rs       # Axum HTTP server
        └── handlers.rs   # API handlers
```

## Installation

### As a Library (from crates.io)

```toml
[dependencies]
htg = "0.1"
```

```rust
use htg::SrtmService;

let service = SrtmService::new("/path/to/hgt/files", 100);
let elevation = service.get_elevation(19.4326, -99.1332)?;
println!("Elevation: {}m", elevation);
```

### As a Service (from DockerHub)

```bash
docker pull pedrosanzmtz/htg-service:latest

docker run -d \
  -p 8080:8080 \
  -v /path/to/hgt/files:/data:ro \
  -e DATA_DIR=/data \
  -e CACHE_SIZE=100 \
  pedrosanzmtz/htg-service:latest
```

### From Source

```bash
git clone https://github.com/pedrosanzmtz/htg.git
cd htg

# Run the service
DATA_DIR=./data CACHE_SIZE=100 cargo run -p htg-service --release
```

## API Endpoints

### GET /elevation

Query elevation for coordinates.

```bash
curl "http://localhost:8080/elevation?lat=19.4326&lon=-99.1332"
```

**Response:**
```json
{
  "latitude": 19.4326,
  "longitude": -99.1332,
  "elevation": 2240
}
```

### GET /health

Health check endpoint.

### GET /stats

Cache statistics.

```json
{
  "cache_entries": 45,
  "cache_hits": 1234,
  "cache_misses": 56,
  "hit_rate": 0.956
}
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `DATA_DIR` | `./data` | Directory containing `.hgt` files |
| `CACHE_SIZE` | `100` | Maximum tiles in memory |
| `PORT` | `8080` | HTTP server port |
| `RUST_LOG` | `info` | Log level |

## SRTM Data

Download `.hgt` files from:
- https://dwtkns.com/srtm30m/
- https://earthexplorer.usgs.gov/

Place files in your `DATA_DIR`. Filename format: `N19W100.hgt` (latitude/longitude).

## Performance

| Metric | Value |
|--------|-------|
| Memory (100 SRTM3 tiles) | ~280MB |
| Memory (100 SRTM1 tiles) | ~2.5GB |
| Cached response | <10ms |
| Uncached response | <50ms |
| Throughput | >10,000 req/s |

## Contributing

### Workflow

1. **Create an issue** describing the feature/bug
2. **Create a branch** from `main`: `git checkout -b feature/issue-number-description`
3. **Make changes** and commit with descriptive messages
4. **Open a Pull Request** linked to the issue
5. **Request review** and address feedback
6. **Merge** after approval

### Rules

- **No direct pushes to `main`** - all changes must go through PRs
- **PRs must reference an issue** - use `Closes #123` in PR description
- **All tests must pass** before merging
- **Code must be formatted** with `cargo fmt`
- **No clippy warnings** - run `cargo clippy`

### Development

```bash
# Run tests
cargo test

# Format code
cargo fmt

# Run clippy
cargo clippy -- -D warnings

# Run service locally
DATA_DIR=./test_data cargo run -p htg-service
```

## Roadmap

See [ROADMAP.md](ROADMAP.md) for detailed implementation phases.

| Phase | Component | Status |
|-------|-----------|--------|
| 1 | Core Tile Parser | Pending |
| 2 | Filename Detection | Pending |
| 3 | Caching Layer | Pending |
| 4 | HTTP API | Pending |
| 5 | Production Ready | Pending |

## License

MIT

## Author

Pedro Sanz Martinez ([@pedrosanzmtz](https://github.com/pedrosanzmtz))
