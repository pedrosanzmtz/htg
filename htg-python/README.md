# htg - High-performance SRTM Elevation Library

[![PyPI](https://img.shields.io/pypi/v/htg.svg)](https://pypi.org/project/htg/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Ultra-fast SRTM elevation queries in Python.** Built with Rust, delivering **3.5x faster** performance than the most popular Python SRTM library.

Python bindings for the [htg](https://github.com/pedrosanzmtz/htg) Rust library, providing blazingly fast elevation queries from SRTM .hgt files with sub-microsecond latency.

## Installation

```bash
pip install srtm
```

## Quick Start

```python
import srtm

# Create service with up to 100 cached tiles
service = srtm.SrtmService("/path/to/srtm", cache_size=100)

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
import srtm

# Convert coordinates to filename
filename = srtm.lat_lon_to_filename(35.5, 138.7)
print(filename)  # "N35E138.hgt"

# Parse filename to coordinates
coords = srtm.filename_to_lat_lon("N35E138.hgt")
print(coords)  # (35, 138)

# Void value constant
print(srtm.VOID_VALUE)  # -32768
```

## SRTM Data

Download SRTM .hgt files from:
- https://dwtkns.com/srtm30m/
- https://earthexplorer.usgs.gov/

## Performance

htg-python delivers **exceptional performance** through its Rust core and PyO3 bindings, significantly outperforming traditional Python SRTM libraries.

### Benchmarks vs Popular Python Libraries

Comparison using **local .hgt files only** (fair, apples-to-apples test):

| Library | Implementation | Per Query | Throughput | vs htg-python |
|---------|----------------|-----------|------------|---------------|
| **htg-python** | Rust + PyO3 | **0.41 μs** | **2,419,110 q/s** | 1.0x (baseline) ⚡ |
| **[srtm.py](https://github.com/tkrajina/srtm.py)** | Pure Python (256 ⭐) | 1.43 μs | 697,654 q/s | **3.5x slower** |
| **[srtm4](https://github.com/centreborelli/srtm4)** | Python + C++ subprocess | 99,630 μs | 10 q/s | **241,017x slower** |

**Key findings:**
- **3.5x faster** than srtm.py (most popular, fair comparison)
- **241,000x faster** than srtm4 (subprocess overhead dominates)
- **Sub-microsecond latency** - queries complete in 0.41 microseconds
- **2.4 million queries/second** on a single thread

*Benchmark environment: Python 3.12, macOS (Apple Silicon). See [BENCHMARKS.md](../BENCHMARKS.md) for full methodology.*

### Why So Fast?

- **Zero-copy memory access**: Memory-mapped I/O eliminates data copying
- **No subprocess overhead**: Direct Rust function calls via PyO3 (unlike srtm4's subprocess approach)
- **Optimized compilation**: LLVM optimizations with inline expansion
- **Efficient caching**: In-memory LRU cache vs disk-based caching

### Real-World Performance

```python
import srtm
service = srtm.SrtmService("/path/to/data", cache_size=100)

# Single query: ~0.4 microseconds (sub-millisecond!)
elevation = service.get_elevation(35.3606, 138.7274)

# Batch queries: ~147k per second on a single thread
for lat, lon in coordinates:
    elevation = service.get_elevation(lat, lon)
```

**Production-ready**: Can handle millions of requests per second with multiple cores.

## License

MIT
