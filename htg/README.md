# htg

[![Crates.io](https://img.shields.io/crates/v/htg.svg)](https://crates.io/crates/htg)
[![Documentation](https://docs.rs/htg/badge.svg)](https://docs.rs/htg)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

High-performance, memory-efficient Rust library for querying elevation data from SRTM (Shuttle Radar Topography Mission) `.hgt` files.

## Features

- **Fast**: Memory-mapped I/O for <10ms lookups
- **Memory Efficient**: LRU cache keeps memory bounded
- **Offline**: Works with local `.hgt` files
- **Auto-Download**: Optional automatic tile download (enable `download` feature)
- **Bilinear Interpolation**: Sub-pixel accuracy for smooth elevation profiles
- **Floor Rounding Mode**: srtm.py-compatible grid cell selection

## Installation

```toml
[dependencies]
htg = "0.2"

# With auto-download support
htg = { version = "0.2", features = ["download"] }
```

## Quick Start

```rust
use htg::SrtmService;

// Create service with data directory and cache size
let service = SrtmService::new("/path/to/hgt/files", 100);

// Query elevation (returns meters as i16)
let elevation = service.get_elevation(35.6762, 139.6503)?;
println!("Elevation: {}m", elevation);

// Query with bilinear interpolation (returns Option<f64>)
if let Some(elevation) = service.get_elevation_interpolated(35.6762, 139.6503)? {
    println!("Interpolated: {:.1}m", elevation);
}

// Floor-based rounding (srtm.py compatible)
let elevation = service.get_elevation_floor(35.6762, 139.6503)?;
```

## Auto-Download

Enable the `download` feature to automatically fetch missing tiles:

```rust
use htg::{SrtmServiceBuilder, download::DownloadConfig};

// Using ArduPilot terrain server (recommended)
let service = SrtmServiceBuilder::new("/data/srtm")
    .cache_size(100)
    .auto_download(DownloadConfig::ardupilot())
    .build()?;

// Tiles are downloaded automatically when needed
let elevation = service.get_elevation(35.6762, 139.6503)?;
```

### Custom Download Source

```rust
use htg::{SrtmServiceBuilder, download::DownloadConfig};

let service = SrtmServiceBuilder::new("/data/srtm")
    .auto_download(DownloadConfig::with_url_template(
        "https://example.com/srtm/{filename}.hgt.gz",
    ))
    .build()?;
```

## Environment Configuration

```rust
use htg::SrtmServiceBuilder;

// Configure via environment variables:
// - HTG_DATA_DIR: Directory containing .hgt files (required)
// - HTG_CACHE_SIZE: Max tiles in cache (default: 100)
// - HTG_DOWNLOAD_SOURCE: "ardupilot", "ardupilot-srtm1", or "ardupilot-srtm3"

let service = SrtmServiceBuilder::from_env()?.build()?;
```

## SRTM Data Format

- **SRTM1**: 1 arc-second (~30m resolution), 3601×3601 samples, ~25MB/tile
- **SRTM3**: 3 arc-second (~90m resolution), 1201×1201 samples, ~2.8MB/tile
- **Coverage**: Global between ±60° latitude
- **Filename**: `N35E138.hgt` (latitude + longitude of SW corner)

### Data Sources

- [SRTM Tile Grabber](https://dwtkns.com/srtm30m/) - Interactive map
- [USGS Earth Explorer](https://earthexplorer.usgs.gov/) - Official source
- [ArduPilot Terrain](https://terrain.ardupilot.org/) - Auto-download source

## Related Crates

This is part of the [htg workspace](https://github.com/pedrosanzmtz/htg):

- **htg** (this crate) - Core library
- **htg-service** - HTTP microservice ([DockerHub](https://hub.docker.com/r/pedropan1995/htg-service))
- **htg-cli** - Command-line tool

## License

MIT
