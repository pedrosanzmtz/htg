#!/usr/bin/env python3
"""
HTG vs SRTM4 Performance Comparison

Benchmarks htg (Rust) against srtm4 (Python/C++) library to quantify
performance improvements in memory usage, latency, and throughput.

Usage:
    python benchmark_comparison.py --data-dir /path/to/srtm [--output results.json]

Requirements:
    pip install srtm4 htg psutil rich

Note: This script requires both libraries to be installed. If htg is not published
      yet, install it locally: cd htg-python && pip install -e .
"""

import argparse
import gc
import json
import os
import statistics
import sys
import time
import tracemalloc
from dataclasses import asdict, dataclass
from typing import Any, Callable

try:
    import psutil
    from rich.console import Console
    from rich.table import Table
except ImportError:
    print("Error: Missing dependencies. Install with:")
    print("  pip install psutil rich")
    sys.exit(1)

console = Console()


@dataclass
class BenchmarkResult:
    """Results for a single benchmark."""

    library: str
    test_name: str
    metric: str
    value: float
    unit: str
    details: dict[str, Any] | None = None


@dataclass
class ComparisonResults:
    """Complete comparison results."""

    memory: dict[str, dict[str, float]]
    latency: dict[str, dict[str, float]]
    throughput: dict[str, dict[str, float]]
    startup: dict[str, dict[str, float]]


# Test coordinates spread across multiple tiles
TEST_COORDS = [
    (35.3606, 138.7274),  # Mount Fuji, Japan
    (27.9881, 86.9250),   # Mount Everest, Nepal
    (45.8326, 6.8652),    # Mont Blanc, France
    (47.5574, 10.7467),   # Zugspitze, Germany
    (-6.0676, 37.3556),   # Mount Kilimanjaro, Tanzania
    (38.8404, -77.0428),  # Washington DC, USA
    (51.5074, -0.1278),   # London, UK
    (35.6762, 139.6503),  # Tokyo, Japan
    (40.7128, -74.0060),  # New York, USA
    (-33.8688, 151.2093), # Sydney, Australia
]


def get_process_memory_mb() -> float:
    """Get current process memory usage in MB."""
    process = psutil.Process()
    return process.memory_info().rss / (1024 * 1024)


def benchmark_srtm4_startup(data_dir: str) -> float:
    """Measure srtm4 import and first query time."""
    # Clear module cache
    if "srtm4" in sys.modules:
        del sys.modules["srtm4"]

    gc.collect()

    start = time.perf_counter()
    import srtm4

    # First query (triggers data download/cache initialization)
    lat, lon = TEST_COORDS[0]
    _ = srtm4.srtm4(lon, lat)  # Note: srtm4 uses (lon, lat) order

    elapsed = time.perf_counter() - start
    return elapsed * 1000  # Convert to ms


def benchmark_htg_startup(data_dir: str, cache_size: int = 100) -> float:
    """Measure htg import and first query time."""
    # Clear module cache
    if "srtm" in sys.modules:
        del sys.modules["srtm"]

    gc.collect()

    start = time.perf_counter()
    import srtm

    # Create service and first query
    service = srtm.SrtmService(data_dir, cache_size=cache_size)
    lat, lon = TEST_COORDS[0]
    _ = service.get_elevation(lat, lon)

    elapsed = time.perf_counter() - start
    return elapsed * 1000  # Convert to ms


def benchmark_srtm4_memory(data_dir: str, num_tiles: int = 10) -> dict[str, float]:
    """Measure srtm4 memory usage."""
    try:
        import srtm4
    except ImportError:
        console.print("[yellow]srtm4 not installed, skipping[/yellow]")
        return {"baseline_mb": 0, "after_queries_mb": 0, "delta_mb": 0}

    gc.collect()
    baseline_mem = get_process_memory_mb()

    # Query multiple tiles
    for i in range(num_tiles):
        lat, lon = TEST_COORDS[i % len(TEST_COORDS)]
        try:
            _ = srtm4.srtm4(lon, lat)  # Note: (lon, lat) order
        except Exception:
            pass  # Ignore missing tiles

    gc.collect()
    after_mem = get_process_memory_mb()

    return {
        "baseline_mb": baseline_mem,
        "after_queries_mb": after_mem,
        "delta_mb": after_mem - baseline_mem,
    }


def benchmark_htg_memory(data_dir: str, num_tiles: int = 10, cache_size: int = 100) -> dict[str, float]:
    """Measure htg memory usage."""
    try:
        import srtm
    except ImportError:
        console.print("[yellow]htg not installed, skipping[/yellow]")
        return {"baseline_mb": 0, "after_queries_mb": 0, "delta_mb": 0}

    gc.collect()
    baseline_mem = get_process_memory_mb()

    service = srtm.SrtmService(data_dir, cache_size=cache_size)

    # Query multiple tiles
    for i in range(num_tiles):
        lat, lon = TEST_COORDS[i % len(TEST_COORDS)]
        try:
            _ = service.get_elevation(lat, lon)
        except Exception:
            pass  # Ignore missing tiles

    gc.collect()
    after_mem = get_process_memory_mb()

    return {
        "baseline_mb": baseline_mem,
        "after_queries_mb": after_mem,
        "delta_mb": after_mem - baseline_mem,
    }


def benchmark_srtm4_latency(data_dir: str, n_queries: int = 1000) -> dict[str, float]:
    """Measure srtm4 query latency."""
    try:
        import srtm4
    except ImportError:
        return {"mean_ms": 0, "p50_ms": 0, "p95_ms": 0, "p99_ms": 0, "min_ms": 0, "max_ms": 0}

    # Warm up (query first tile)
    lat, lon = TEST_COORDS[0]
    _ = srtm4.srtm4(lon, lat)

    # Measure latencies
    latencies = []
    for i in range(n_queries):
        lat, lon = TEST_COORDS[i % len(TEST_COORDS)]

        start = time.perf_counter()
        try:
            _ = srtm4.srtm4(lon, lat)
            elapsed = (time.perf_counter() - start) * 1000
            latencies.append(elapsed)
        except Exception:
            pass

    if not latencies:
        return {"mean_ms": 0, "p50_ms": 0, "p95_ms": 0, "p99_ms": 0, "min_ms": 0, "max_ms": 0}

    latencies.sort()
    n = len(latencies)

    return {
        "mean_ms": statistics.mean(latencies),
        "p50_ms": latencies[n // 2],
        "p95_ms": latencies[int(n * 0.95)],
        "p99_ms": latencies[int(n * 0.99)],
        "min_ms": min(latencies),
        "max_ms": max(latencies),
    }


def benchmark_htg_latency(data_dir: str, n_queries: int = 1000, cache_size: int = 100) -> dict[str, float]:
    """Measure htg query latency."""
    try:
        import srtm
    except ImportError:
        return {"mean_ms": 0, "p50_ms": 0, "p95_ms": 0, "p99_ms": 0, "min_ms": 0, "max_ms": 0}

    service = srtm.SrtmService(data_dir, cache_size=cache_size)

    # Warm up (query first tile)
    lat, lon = TEST_COORDS[0]
    _ = service.get_elevation(lat, lon)

    # Measure latencies
    latencies = []
    for i in range(n_queries):
        lat, lon = TEST_COORDS[i % len(TEST_COORDS)]

        start = time.perf_counter()
        try:
            _ = service.get_elevation(lat, lon)
            elapsed = (time.perf_counter() - start) * 1000
            latencies.append(elapsed)
        except Exception:
            pass

    if not latencies:
        return {"mean_ms": 0, "p50_ms": 0, "p95_ms": 0, "p99_ms": 0, "min_ms": 0, "max_ms": 0}

    latencies.sort()
    n = len(latencies)

    return {
        "mean_ms": statistics.mean(latencies),
        "p50_ms": latencies[n // 2],
        "p95_ms": latencies[int(n * 0.95)],
        "p99_ms": latencies[int(n * 0.99)],
        "min_ms": min(latencies),
        "max_ms": max(latencies),
    }


def benchmark_srtm4_throughput(data_dir: str, duration_seconds: int = 5) -> dict[str, float]:
    """Measure srtm4 throughput (single-threaded)."""
    try:
        import srtm4
    except ImportError:
        return {"queries_per_second": 0, "total_queries": 0}

    # Warm up
    lat, lon = TEST_COORDS[0]
    _ = srtm4.srtm4(lon, lat)

    count = 0
    end_time = time.time() + duration_seconds
    coord_idx = 0

    while time.time() < end_time:
        lat, lon = TEST_COORDS[coord_idx % len(TEST_COORDS)]
        try:
            _ = srtm4.srtm4(lon, lat)
            count += 1
        except Exception:
            pass
        coord_idx += 1

    qps = count / duration_seconds
    return {
        "queries_per_second": qps,
        "total_queries": count,
    }


def benchmark_htg_throughput(data_dir: str, duration_seconds: int = 5, cache_size: int = 100) -> dict[str, float]:
    """Measure htg throughput (single-threaded)."""
    try:
        import srtm
    except ImportError:
        return {"queries_per_second": 0, "total_queries": 0}

    service = srtm.SrtmService(data_dir, cache_size=cache_size)

    # Warm up
    lat, lon = TEST_COORDS[0]
    _ = service.get_elevation(lat, lon)

    count = 0
    end_time = time.time() + duration_seconds
    coord_idx = 0

    while time.time() < end_time:
        lat, lon = TEST_COORDS[coord_idx % len(TEST_COORDS)]
        try:
            _ = service.get_elevation(lat, lon)
            count += 1
        except Exception:
            pass
        coord_idx += 1

    qps = count / duration_seconds
    return {
        "queries_per_second": qps,
        "total_queries": count,
    }


def print_comparison_table(title: str, srtm4_results: dict, htg_results: dict, metric_names: list[str], unit: str):
    """Print a formatted comparison table."""
    table = Table(title=title)
    table.add_column("Metric", style="cyan")
    table.add_column("srtm4", style="yellow")
    table.add_column("htg", style="green")
    table.add_column("Improvement", style="magenta")

    for metric in metric_names:
        srtm4_val = srtm4_results.get(metric, 0)
        htg_val = htg_results.get(metric, 0)

        # Calculate improvement
        if srtm4_val > 0 and htg_val > 0:
            if "throughput" in metric or "qps" in metric or "queries" in metric:
                # Higher is better
                improvement = f"{htg_val / srtm4_val:.2f}x faster"
            else:
                # Lower is better
                improvement = f"{srtm4_val / htg_val:.2f}x faster"
        else:
            improvement = "N/A"

        table.add_row(
            metric.replace("_", " ").title(),
            f"{srtm4_val:.2f} {unit}",
            f"{htg_val:.2f} {unit}",
            improvement,
        )

    console.print(table)
    console.print()


def run_benchmarks(data_dir: str, output_file: str | None = None):
    """Run complete benchmark suite."""
    console.print("\n[bold blue]=== HTG vs SRTM4 Performance Comparison ===[/bold blue]\n")

    if not os.path.isdir(data_dir):
        console.print(f"[red]Error: Data directory not found: {data_dir}[/red]")
        return

    # Check for required libraries
    try:
        import srtm4
        has_srtm4 = True
    except ImportError:
        console.print("[yellow]Warning: srtm4 not installed. Install with: pip install srtm4[/yellow]")
        has_srtm4 = False

    try:
        import srtm
        has_htg = True
    except ImportError:
        console.print("[yellow]Warning: htg not installed. Install with: pip install htg[/yellow]")
        console.print("[yellow]Or install locally: cd htg-python && pip install -e .[/yellow]")
        has_htg = False

    if not has_srtm4 and not has_htg:
        console.print("[red]Error: Neither library is installed. Cannot run benchmarks.[/red]")
        return

    results = {}

    # --- Startup Time ---
    console.print("[bold]1. Startup Time (import + first query)[/bold]")
    srtm4_startup = benchmark_srtm4_startup(data_dir) if has_srtm4 else 0
    htg_startup = benchmark_htg_startup(data_dir) if has_htg else 0

    results["startup"] = {
        "srtm4": {"startup_time_ms": srtm4_startup},
        "htg": {"startup_time_ms": htg_startup},
    }

    if has_srtm4 and has_htg:
        print_comparison_table(
            "Startup Time",
            results["startup"]["srtm4"],
            results["startup"]["htg"],
            ["startup_time_ms"],
            "ms"
        )

    # --- Memory Usage ---
    console.print("[bold]2. Memory Usage (10 tiles)[/bold]")
    srtm4_mem = benchmark_srtm4_memory(data_dir, num_tiles=10) if has_srtm4 else {}
    htg_mem = benchmark_htg_memory(data_dir, num_tiles=10) if has_htg else {}

    results["memory"] = {
        "srtm4": srtm4_mem,
        "htg": htg_mem,
    }

    if has_srtm4 and has_htg:
        print_comparison_table(
            "Memory Usage",
            srtm4_mem,
            htg_mem,
            ["baseline_mb", "after_queries_mb", "delta_mb"],
            "MB"
        )

    # --- Query Latency ---
    console.print("[bold]3. Query Latency (1000 warm queries)[/bold]")
    srtm4_latency = benchmark_srtm4_latency(data_dir, n_queries=1000) if has_srtm4 else {}
    htg_latency = benchmark_htg_latency(data_dir, n_queries=1000) if has_htg else {}

    results["latency"] = {
        "srtm4": srtm4_latency,
        "htg": htg_latency,
    }

    if has_srtm4 and has_htg:
        print_comparison_table(
            "Query Latency",
            srtm4_latency,
            htg_latency,
            ["mean_ms", "p50_ms", "p95_ms", "p99_ms"],
            "ms"
        )

    # --- Throughput ---
    console.print("[bold]4. Throughput (5 second test)[/bold]")
    srtm4_throughput = benchmark_srtm4_throughput(data_dir, duration_seconds=5) if has_srtm4 else {}
    htg_throughput = benchmark_htg_throughput(data_dir, duration_seconds=5) if has_htg else {}

    results["throughput"] = {
        "srtm4": srtm4_throughput,
        "htg": htg_throughput,
    }

    if has_srtm4 and has_htg:
        print_comparison_table(
            "Throughput",
            srtm4_throughput,
            htg_throughput,
            ["queries_per_second", "total_queries"],
            "queries"
        )

    # --- Summary ---
    if has_srtm4 and has_htg:
        console.print("[bold green]=== Summary ===[/bold green]")

        # Calculate overall improvements
        if srtm4_mem.get("delta_mb", 0) > 0 and htg_mem.get("delta_mb", 0) > 0:
            mem_improvement = srtm4_mem["delta_mb"] / htg_mem["delta_mb"]
            console.print(f"Memory: [green]{mem_improvement:.1f}x lower[/green] usage")

        if srtm4_latency.get("p50_ms", 0) > 0 and htg_latency.get("p50_ms", 0) > 0:
            latency_improvement = srtm4_latency["p50_ms"] / htg_latency["p50_ms"]
            console.print(f"Latency: [green]{latency_improvement:.1f}x faster[/green] (p50)")

        if srtm4_throughput.get("queries_per_second", 0) > 0 and htg_throughput.get("queries_per_second", 0) > 0:
            throughput_improvement = htg_throughput["queries_per_second"] / srtm4_throughput["queries_per_second"]
            console.print(f"Throughput: [green]{throughput_improvement:.1f}x higher[/green]")

        console.print()

    # Save results to JSON if requested
    if output_file:
        with open(output_file, "w") as f:
            json.dump(results, f, indent=2)
        console.print(f"[green]Results saved to {output_file}[/green]")


def main():
    parser = argparse.ArgumentParser(
        description="Compare htg vs srtm4 performance"
    )
    parser.add_argument(
        "--data-dir",
        required=True,
        help="Path to SRTM .hgt files directory",
    )
    parser.add_argument(
        "--output",
        help="Output JSON file for results (optional)",
    )

    args = parser.parse_args()

    run_benchmarks(args.data_dir, args.output)


if __name__ == "__main__":
    main()
