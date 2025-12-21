# HTG - SRTM Elevation Library & Service

[![Crates.io](https://img.shields.io/crates/v/htg.svg)](https://crates.io/crates/htg)
[![Docker Hub](https://img.shields.io/docker/v/pedropan1995/htg-service?label=docker)](https://hub.docker.com/r/pedropan1995/htg-service)
[![CI](https://github.com/pedrosanzmtz/htg/actions/workflows/ci.yml/badge.svg)](https://github.com/pedrosanzmtz/htg/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

High-performance, memory-efficient Rust library and microservice for querying elevation data from SRTM (Shuttle Radar Topography Mission) `.hgt` files.

## Problem

Existing elevation services (e.g., Python/Flask) consume excessive memory (7GB+). This project provides a Rust-based solution using **<100MB** with the same functionality.

## Features

- **Fast**: <10ms response time for cached tiles
- **Memory Efficient**: <100MB with 100 cached tiles (vs 7GB in Python)
- **Offline**: No internet required, works with local `.hgt` files
- **Auto-Download**: Optional automatic tile download from configurable sources
- **Automatic Detection**: Determines correct tile from coordinates
- **LRU Caching**: Configurable cache size to bound memory usage
- **Docker Ready**: Easy deployment with Docker/Docker Compose
- **OpenAPI Docs**: Interactive Swagger UI at `/docs`

## Project Structure

This is a Cargo workspace with three crates:

```
htg/
├── htg/              # Library crate (publish to crates.io)
│   └── src/
│       ├── lib.rs
│       ├── tile.rs       # SRTM tile parsing
│       ├── filename.rs   # Coordinate → filename conversion
│       ├── service.rs    # Caching service
│       ├── download.rs   # Auto-download functionality
│       └── error.rs      # Error types
│
├── htg-service/      # HTTP service binary (publish to DockerHub)
│   └── src/
│       ├── main.rs       # Axum HTTP server
│       └── handlers.rs   # API handlers
│
└── htg-cli/          # CLI tool binary
    └── src/
        ├── main.rs       # CLI entry point
        └── commands/     # Command implementations
```

## Quick Start

### Using Docker (Recommended)

```bash
# Clone the repository
git clone https://github.com/pedrosanzmtz/htg.git
cd htg

# Create data directory and add .hgt files
mkdir -p data/srtm
# Copy your .hgt files to data/srtm/

# Run with Docker Compose
docker compose up -d

# Test it
curl "http://localhost:8080/elevation?lat=35.6762&lon=139.6503"
```

### Using Docker Hub

```bash
docker run -d \
  -p 8080:8080 \
  -v /path/to/hgt/files:/data/srtm:ro \
  -e HTG_DATA_DIR=/data/srtm \
  -e HTG_CACHE_SIZE=100 \
  pedropan1995/htg-service:latest
```

### From Source

```bash
git clone https://github.com/pedrosanzmtz/htg.git
cd htg

# Run the service
HTG_DATA_DIR=./data/srtm cargo run -p htg-service --release
```

## API Endpoints

### GET /elevation

Query elevation for coordinates.

**Request:**
```bash
curl "http://localhost:8080/elevation?lat=35.6762&lon=139.6503"
```

**Response (200 OK):**
```json
{
  "elevation": 40,
  "lat": 35.6762,
  "lon": 139.6503
}
```

**Error Response (400 Bad Request):**
```json
{
  "error": "Coordinates out of bounds: lat=91, lon=0 (valid: lat ±60°, lon ±180°)"
}
```

**Error Response (404 Not Found):**
```json
{
  "error": "Tile not available: N35E139.hgt (not found locally, auto-download disabled)"
}
```

### GET /health

Health check endpoint.

**Response:**
```json
{
  "status": "healthy",
  "version": "0.1.0"
}
```

### GET /stats

Cache statistics.

**Response:**
```json
{
  "cached_tiles": 45,
  "cache_hits": 1234,
  "cache_misses": 56,
  "hit_rate": 0.956
}
```

### GET /docs

Interactive OpenAPI documentation (Swagger UI).

Open in browser: `http://localhost:8080/docs`

The OpenAPI JSON spec is available at `/api-docs/openapi.json`.

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `HTG_DATA_DIR` | `.` | Directory containing `.hgt` files |
| `HTG_CACHE_SIZE` | `100` | Maximum tiles in memory |
| `HTG_PORT` | `8080` | HTTP server port |
| `HTG_DOWNLOAD_SOURCE` | - | Named source: "ardupilot", "ardupilot-srtm1", "ardupilot-srtm3" |
| `HTG_DOWNLOAD_URL` | - | URL template for auto-download (optional) |
| `HTG_DOWNLOAD_GZIP` | `false` | Whether downloaded files are gzipped |
| `RUST_LOG` | `info` | Log level (debug, info, warn, error) |

### Auto-Download Configuration

#### Using ArduPilot (Recommended)

The easiest way to enable auto-download is using the ArduPilot terrain server:

```bash
# SRTM1 - High resolution (30m, ~25MB/tile) - recommended
export HTG_DOWNLOAD_SOURCE=ardupilot

# SRTM3 - Lower resolution (90m, ~2.8MB/tile) - faster downloads
export HTG_DOWNLOAD_SOURCE=ardupilot-srtm3
```

This automatically downloads tiles from `https://terrain.ardupilot.org/`.

#### Using Custom URL Template

For other data sources, use a custom URL template:

```bash
export HTG_DOWNLOAD_URL="https://example.com/srtm/{filename}.hgt.gz"
```

**URL Template Placeholders:**
- `{filename}` - Full filename (e.g., "N35E138")
- `{lat_prefix}` - N or S
- `{lat}` - Latitude digits (e.g., "35")
- `{lon_prefix}` - E or W
- `{lon}` - Longitude digits (e.g., "138")
- `{continent}` - Continent subdirectory (e.g., "Eurasia", "North_America")

## Library Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
htg = "0.1"

# With auto-download support
htg = { version = "0.1", features = ["download"] }
```

### Basic Usage

```rust
use htg::SrtmService;

let service = SrtmService::new("/path/to/hgt/files", 100);
let elevation = service.get_elevation(35.6762, 139.6503)?;
println!("Elevation: {}m", elevation);
```

### With Auto-Download (ArduPilot)

```rust
use htg::{SrtmServiceBuilder, download::DownloadConfig};

let service = SrtmServiceBuilder::new("/data/srtm")
    .cache_size(100)
    .auto_download(DownloadConfig::ardupilot())
    .build()?;

// Will download N35E139.hgt from ArduPilot if not present locally
let elevation = service.get_elevation(35.6762, 139.6503)?;
```

### With Custom URL Template

```rust
use htg::{SrtmServiceBuilder, download::DownloadConfig};

let service = SrtmServiceBuilder::new("/data/srtm")
    .cache_size(100)
    .auto_download(DownloadConfig::with_url_template(
        "https://example.com/srtm/{filename}.hgt.gz",
    ))
    .build()?;

// Will download N35E139.hgt if not present locally
let elevation = service.get_elevation(35.6762, 139.6503)?;
```

### From Environment Variables

```rust
use htg::SrtmServiceBuilder;

let service = SrtmServiceBuilder::from_env()?.build()?;
let elevation = service.get_elevation(35.6762, 139.6503)?;
```

## CLI Tool

The `htg` CLI tool provides offline elevation queries from the command line.

### Installation

```bash
# From source
cargo install --path htg-cli

# Or build and run directly
cargo run -p htg-cli -- --help
```

### Commands

#### Query (Single Point)

```bash
# Basic query
htg query --lat 35.3606 --lon 138.7274
# Output: 3776

# With interpolation
htg query --lat 35.3606 --lon 138.7274 --interpolate
# Output: 3776.42

# JSON output
htg query --lat 35.3606 --lon 138.7274 --json
# Output: {"lat":35.3606,"lon":138.7274,"elevation":3776.0,"interpolated":false}
```

#### Batch (CSV/GeoJSON)

```bash
# Process CSV file (adds elevation column)
htg batch input.csv --output output.csv

# Custom column names
htg batch input.csv --lat-col latitude --lon-col longitude

# Process GeoJSON (adds Z coordinate)
htg batch input.geojson --output output.geojson
```

#### Info (Tile Information)

```bash
# By tile name
htg info N35E138

# By file path
htg info /path/to/N35E138.hgt

# Output:
# Tile: N35E138.hgt
# Path: /data/srtm/N35E138.hgt
#
# Resolution: SRTM3 (~90m) (1201x1201 samples)
# Coverage: N35-N36, E138E139
# File size: 2.88 MB
#
# Min elevation: -12m
# Max elevation: 3776m
```

#### List (Available Tiles)

```bash
htg list
# Output:
# TILE           TYPE             COVERAGE
# --------------------------------------------
# N35E138.hgt   SRTM3    N35 to N36, E138 to E139
# N35E139.hgt   SRTM3    N35 to N36, E139 to E140
# ...
#
# Summary:
#   Total tiles: 2
#   SRTM3 (90m): 2
#   Total size: 5.77 MB
```

### Global Options

```bash
htg --data-dir /path/to/srtm --cache-size 50 --auto-download query --lat 35.5 --lon 138.5
```

| Option | Description |
|--------|-------------|
| `-d, --data-dir` | Directory containing .hgt files (or set `HTG_DATA_DIR`) |
| `-c, --cache-size` | Maximum tiles in cache (default: 100) |
| `-a, --auto-download` | Enable automatic tile download from ArduPilot |

## SRTM Data

### Data Format

- **SRTM1**: 3601×3601 samples, 1 arc-second (~30m) resolution, ~25MB per tile
- **SRTM3**: 1201×1201 samples, 3 arc-second (~90m) resolution, ~2.8MB per tile
- **Coverage**: ±60° latitude globally
- **Filename**: `N35E138.hgt` (latitude prefix + latitude + longitude prefix + longitude)

### Download Sources

- [SRTM Tile Grabber](https://dwtkns.com/srtm30m/) - Interactive map to download tiles
- [USGS Earth Explorer](https://earthexplorer.usgs.gov/) - Official source
- [OpenTopography](https://opentopography.org/) - Academic/research access

Place downloaded `.hgt` files in your `HTG_DATA_DIR` directory.

## Performance

| Metric | Value |
|--------|-------|
| Memory (100 SRTM3 tiles) | ~280MB |
| Memory (100 SRTM1 tiles) | ~2.5GB |
| Cached response | <10ms |
| Uncached response | <50ms |
| Throughput | >10,000 req/s |

## Development

### Prerequisites

- Rust 1.75 or later
- Docker (optional, for containerized deployment)

### Commands

```bash
# Run tests
cargo test --workspace

# Run tests with download feature
cargo test --workspace --features download

# Format code
cargo fmt --all

# Run clippy
cargo clippy --workspace -- -D warnings

# Build release
cargo build --release -p htg-service

# Run service locally
HTG_DATA_DIR=./data/srtm cargo run -p htg-service
```

### Docker Build

```bash
# Build image
docker build -t htg-service .

# Run container
docker run -d -p 8080:8080 -v ./data/srtm:/data/srtm:ro htg-service
```

## Benchmarks

Run performance benchmarks to validate memory usage, latency, and throughput.

### Prerequisites

```bash
pip install -r benchmarks/requirements.txt
```

### Running Benchmarks

```bash
# Create synthetic test tiles (100 SRTM3 tiles)
python benchmarks/create_test_tiles.py --num-tiles 100

# Start the service in Docker
docker compose -f benchmarks/docker-compose.bench.yml up -d

# Wait for service to start
sleep 10

# Run benchmarks
python benchmarks/benchmark.py --url http://localhost:8080

# Stop the service
docker compose -f benchmarks/docker-compose.bench.yml down
```

### Expected Output

```
=== HTG Performance Benchmark ===

Memory Usage:
  Baseline:     12 MB
  10 tiles:     42 MB
  50 tiles:     78 MB
  100 tiles:    95 MB PASS (target: <100MB)

Latency (1000 requests):
  Warm cache:   0.8ms (p50), 1.2ms (p95), 2.1ms (p99) PASS (target: <10ms)

Throughput:
  Single tile:  15,234 req/sec PASS (target: >1000)

GeoJSON Batch:
  10 points:    2ms
  100 points:   12ms
  1000 points:  89ms
```

### Performance Targets

| Metric | Target | Description |
|--------|--------|-------------|
| Memory (100 tiles) | <100MB | With 100 SRTM3 tiles cached |
| Cached latency | <10ms | Repeated queries to same tile |
| Uncached latency | <50ms | First query to new tile |
| Throughput | >1000 req/s | Sustained request rate |

## Contributing

### Workflow

1. **Create an issue** describing the feature/bug
2. **Create a branch** from `main`: `git checkout -b feature/issue-number-description`
3. **Make changes** and commit with descriptive messages
4. **Open a Pull Request** linked to the issue
5. **Wait for CI** - all checks must pass
6. **Merge** after approval

### Rules

- **No direct pushes to `main`** - all changes must go through PRs
- **PRs must reference an issue** - use `Closes #123` in PR description
- **All tests must pass** before merging
- **Code must be formatted** with `cargo fmt`
- **No clippy warnings** - run `cargo clippy`

## Roadmap

| Phase | Component | Status |
|-------|-----------|--------|
| 1 | Core Tile Parser | ✅ Complete |
| 2 | Filename Detection | ✅ Complete |
| 3 | Caching Layer | ✅ Complete |
| 4 | HTTP API | ✅ Complete |
| 5 | Production Ready | ✅ Complete |
| 6 | Publish to crates.io | 🔄 Pending |
| 7 | Publish to DockerHub | 🔄 Pending |
| 8 | CGIAR SRTM v4 support | 📋 Planned |
| 9 | Bicubic interpolation | 📋 Planned |

### Coming Soon: SRTM v4 Support

We're adding support for [CGIAR SRTM v4](https://bigdata.cgiar.org/srtm-90m-digital-elevation-database/) data, which provides:

- **Void-filled data** - No gaps in mountainous or snow-covered regions
- **Higher accuracy** - ~4.5m RMS vs ~6m for raw NASA SRTM
- **Seamless global coverage** - Interpolated peaks and difficult terrain

See issues [#55](https://github.com/pedrosanzmtz/htg/issues/55), [#56](https://github.com/pedrosanzmtz/htg/issues/56), and [#57](https://github.com/pedrosanzmtz/htg/issues/57) for details.

## License

MIT

## Author

Pedro Sanz Martinez ([@pedrosanzmtz](https://github.com/pedrosanzmtz))
