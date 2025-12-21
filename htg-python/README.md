# htg - High-performance SRTM Elevation Library

[![PyPI](https://img.shields.io/pypi/v/htg.svg)](https://pypi.org/project/htg/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Python bindings for the [htg](https://github.com/pedrosanzmtz/htg) Rust library, providing fast elevation queries from SRTM .hgt files.

## Installation

```bash
pip install htg-srtm
```

## Quick Start

```python
import htg

# Create service with up to 100 cached tiles
service = htg.SrtmService("/path/to/srtm", cache_size=100)

# Query elevation (Mount Fuji)
elevation = service.get_elevation(35.3606, 138.7274)
print(f"Elevation: {elevation}m")  # 3776

# Interpolated query for smoother results
elevation = service.get_elevation_interpolated(35.3606, 138.7274)
print(f"Elevation: {elevation:.2f}m")  # 3776.42

# Check cache performance
stats = service.cache_stats()
print(f"Cache hit rate: {stats.hit_rate:.1%}")
```

## Utility Functions

```python
import htg

# Convert coordinates to filename
filename = htg.lat_lon_to_filename(35.5, 138.7)
print(filename)  # "N35E138.hgt"

# Parse filename to coordinates
coords = htg.filename_to_lat_lon("N35E138.hgt")
print(coords)  # (35, 138)

# Void value constant
print(htg.VOID_VALUE)  # -32768
```

## SRTM Data

Download SRTM .hgt files from:
- https://dwtkns.com/srtm30m/
- https://earthexplorer.usgs.gov/

## Performance

This library uses Rust for the core implementation, providing:
- **<10ms** response time for cached tiles
- **Memory-mapped I/O** for fast file access
- **LRU caching** to bound memory usage

## License

MIT
