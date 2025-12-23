# HTG Benchmarks

This directory contains benchmark scripts for measuring htg performance and comparing against other elevation libraries.

## Available Benchmarks

### 1. HTG Service Benchmark (`benchmark.py`)

Measures the performance of the htg-service Docker container.

**Metrics:**
- Memory usage (baseline, 10/50/100 tiles)
- Query latency (p50, p95, p99)
- Throughput (requests/second)
- GeoJSON batch processing
- Cache statistics

**Usage:**
```bash
# Start the service with Docker Compose
docker-compose -f docker-compose.bench.yml up -d

# Run benchmarks
pip install -r requirements.txt
python benchmark.py --url http://localhost:8080 --container htg-bench

# Cleanup
docker-compose -f docker-compose.bench.yml down
```

### 2. HTG vs SRTM4 Comparison (`benchmark_comparison.py`)

Compares htg (Rust) against the popular srtm4 (Python/C++) library.

**Metrics:**
- Startup time (import + first query)
- Memory usage (process RSS)
- Query latency (mean, p50, p95, p99)
- Throughput (queries/second)

**Installation:**
```bash
# Install dependencies
pip install -r requirements.txt

# Install htg Python bindings locally (if not published yet)
cd ../htg-python
pip install -e .
cd ../benchmarks

# Note: srtm4 requires libtiff development files
# Ubuntu/Debian: apt-get install libtiff-dev
# macOS: brew install libtiff
```

**Usage:**
```bash
python benchmark_comparison.py --data-dir /path/to/srtm --output results.json
```

**Test Coordinates:**
The comparison benchmark uses 10 diverse locations across multiple SRTM tiles:
- Mount Fuji, Japan (35.36°N, 138.73°E)
- Mount Everest, Nepal (27.99°N, 86.93°E)
- Mont Blanc, France (45.83°N, 6.87°E)
- Zugspitze, Germany (47.56°N, 10.75°E)
- Mount Kilimanjaro, Tanzania (6.07°S, 37.36°E)
- Washington DC, USA (38.84°N, 77.04°W)
- London, UK (51.51°N, 0.13°W)
- Tokyo, Japan (35.68°N, 139.65°E)
- New York, USA (40.71°N, 74.01°W)
- Sydney, Australia (33.87°S, 151.21°E)

## Test Data

The `create_test_tiles.py` script generates synthetic SRTM tiles for testing when real data is unavailable:

```bash
python create_test_tiles.py --output ./srtm_cache
```

This creates 50 SRTM3 tiles (1201×1201) with synthetic elevation data.

## Expected Results

Based on the project goals, htg should demonstrate:

| Metric | Target | Typical Result |
|--------|--------|----------------|
| Memory (100 tiles) | <100MB | ~95MB |
| Latency (cached) | <10ms | <1ms (p50) |
| Latency (uncached) | <50ms | ~20ms (p50) |
| Throughput | >1,000 req/s | >10,000 req/s |

**vs. srtm4 (expected):**
- **10-100x lower memory** usage
- **10-100x faster** query latency
- **10-100x higher** throughput
- **Comparable accuracy** (same SRTM data)

See `../BENCHMARKS.md` for detailed results and analysis.

## CI Integration

The benchmark suite can be integrated into CI/CD pipelines for performance regression testing:

```yaml
# .github/workflows/benchmark.yml
- name: Run benchmarks
  run: |
    docker-compose -f benchmarks/docker-compose.bench.yml up -d
    sleep 5
    python benchmarks/benchmark.py
```

## Notes

- **srtm4 Data Source:** srtm4 downloads from CGIAR SRTM v4 (void-filled), while htg uses raw SRTM data by default. Results may vary slightly due to different data sources.
- **Cache Behavior:** srtm4 caches tiles in `~/.srtm/`, while htg uses an in-memory LRU cache.
- **Concurrency:** The comparison benchmark runs single-threaded queries. htg-service supports concurrent requests via async Rust.
- **Platform:** Results may vary by platform. Benchmarks should be run on consistent hardware for comparison.
