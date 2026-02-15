# srtm - High-performance SRTM Elevation Library

[![PyPI](https://img.shields.io/pypi/v/srtm.svg)](https://pypi.org/project/srtm/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Ultra-fast SRTM elevation queries in Python.** Built with Rust, delivering **3.5x faster** performance than the most popular Python SRTM library.

Python bindings for the [htg](https://github.com/pedrosanzmtz/htg) Rust library, providing blazingly fast elevation queries from SRTM .hgt files with sub-microsecond latency.

## Installation

```bash
pip install srtm
```

Prebuilt wheels are available for Python 3.12+ on Linux (x86_64, aarch64), macOS (Apple Silicon, x86_64), and Windows (x86_64).

## Quick Start

```python
import srtm

# Create service with up to 100 cached tiles
service = srtm.SrtmService("/path/to/srtm", cache_size=100)

# Query elevation (Mount Fuji)
# Returns None for void data or missing tiles
elevation = service.get_elevation(35.3606, 138.7274)
if elevation is not None:
    print(f"Elevation: {elevation}m")  # 3776

# Interpolated query for smoother results
# Returns None if any surrounding point is void
elevation = service.get_elevation_interpolated(35.3606, 138.7274)
if elevation is not None:
    print(f"Elevation: {elevation:.2f}m")  # 3776.42

# Check cache performance
stats = service.cache_stats()
print(f"Cache hit rate: {stats.hit_rate:.1%}")
```

## Batch Queries

Query multiple coordinates efficiently in a single call:

```python
import srtm

service = srtm.SrtmService("/path/to/srtm", cache_size=100)

coords = [
    (35.3606, 138.7274),  # Mount Fuji
    (27.9881, 86.9250),   # Mount Everest
    (46.8523, 9.1512),    # Piz Bernina
]

# Returns a list of elevations; uses default (0) for void/missing data
elevations = service.get_elevations_batch(coords, default=0)
print(elevations)  # [3776, 8752, 3148]
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

Download SRTM `.hgt` files from:
- https://dwtkns.com/srtm30m/
- https://earthexplorer.usgs.gov/

Both `.hgt` and `.hgt.zip` files are supported. ZIP files are transparently extracted on first access.

## Performance

srtm delivers **exceptional performance** through its Rust core and PyO3 bindings, significantly outperforming traditional Python SRTM libraries.

### Benchmarks vs Popular Python Libraries

Comparison using **local .hgt files only** (fair, apples-to-apples test):

| Library | Implementation | Per Query | Throughput | vs srtm |
|---------|----------------|-----------|------------|---------|
| **srtm** | Rust + PyO3 | **0.41 us** | **2,419,110 q/s** | 1.0x (baseline) |
| **[srtm.py](https://github.com/tkrajina/srtm.py)** | Pure Python (256 stars) | 1.43 us | 697,654 q/s | **3.5x slower** |
| **[srtm4](https://github.com/centreborelli/srtm4)** | Python + C++ subprocess | 99,630 us | 10 q/s | **241,017x slower** |

**Key findings:**
- **3.5x faster** than srtm.py (most popular, fair comparison)
- **241,000x faster** than srtm4 (subprocess overhead dominates)
- **Sub-microsecond latency** - queries complete in 0.41 microseconds
- **2.4 million queries/second** on a single thread

*Benchmark environment: Python 3.12, macOS (Apple Silicon). See [BENCHMARKS.md](https://github.com/pedrosanzmtz/htg/blob/main/BENCHMARKS.md) for full methodology.*

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
