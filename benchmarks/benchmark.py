#!/usr/bin/env python3
"""
HTG Performance Benchmark Suite

Measures memory usage, latency, and throughput of the htg-service
running in a Docker container.

Usage:
    python benchmark.py [--url URL] [--container NAME]

Requirements:
    pip install httpx rich
"""

import argparse
import asyncio
import json
import subprocess
import time
from dataclasses import dataclass

import httpx
from rich.console import Console
from rich.table import Table

console = Console()


@dataclass
class LatencyResults:
    """Latency measurement results."""

    p50: float
    p95: float
    p99: float
    min: float
    max: float
    mean: float
    count: int


@dataclass
class ThroughputResults:
    """Throughput measurement results."""

    requests_per_second: float
    total_requests: int
    duration_seconds: float
    errors: int


@dataclass
class MemoryResults:
    """Memory measurement results."""

    current_mb: float
    limit_mb: float | None


# Success criteria from CLAUDE.md
TARGETS = {
    "memory_100_tiles_mb": 100,
    "latency_cached_ms": 10,
    "latency_uncached_ms": 50,
    "throughput_rps": 1000,
}


def measure_memory(container_name: str) -> MemoryResults | None:
    """Get container memory usage via docker stats."""
    try:
        result = subprocess.run(
            [
                "docker",
                "stats",
                container_name,
                "--no-stream",
                "--format",
                "{{.MemUsage}}",
            ],
            capture_output=True,
            text=True,
            timeout=10,
        )
        if result.returncode != 0:
            return None

        # Parse "95.2MiB / 512MiB" format
        parts = result.stdout.strip().split("/")
        if len(parts) != 2:
            return None

        def parse_mem(s: str) -> float:
            s = s.strip()
            if "GiB" in s:
                return float(s.replace("GiB", "")) * 1024
            elif "MiB" in s:
                return float(s.replace("MiB", ""))
            elif "KiB" in s:
                return float(s.replace("KiB", "")) / 1024
            elif "B" in s:
                return float(s.replace("B", "")) / (1024 * 1024)
            return 0.0

        current = parse_mem(parts[0])
        limit = parse_mem(parts[1])

        return MemoryResults(current_mb=current, limit_mb=limit if limit > 0 else None)
    except (subprocess.TimeoutExpired, FileNotFoundError, ValueError):
        return None


async def measure_latency(
    client: httpx.AsyncClient,
    url: str,
    lat: float,
    lon: float,
    n_requests: int = 1000,
) -> LatencyResults:
    """Measure request latency with percentiles."""
    latencies: list[float] = []

    for _ in range(n_requests):
        start = time.perf_counter()
        response = await client.get(f"{url}/elevation?lat={lat}&lon={lon}")
        elapsed = (time.perf_counter() - start) * 1000  # Convert to ms

        if response.status_code == 200:
            latencies.append(elapsed)

    if not latencies:
        return LatencyResults(
            p50=0, p95=0, p99=0, min=0, max=0, mean=0, count=0
        )

    latencies.sort()
    n = len(latencies)

    return LatencyResults(
        p50=latencies[n // 2],
        p95=latencies[int(n * 0.95)],
        p99=latencies[int(n * 0.99)],
        min=latencies[0],
        max=latencies[-1],
        mean=sum(latencies) / n,
        count=n,
    )


async def measure_throughput(
    client: httpx.AsyncClient,
    url: str,
    lat: float,
    lon: float,
    duration_seconds: int = 10,
    concurrency: int = 10,
) -> ThroughputResults:
    """Measure requests per second with concurrent connections."""
    count = 0
    errors = 0
    end_time = time.time() + duration_seconds

    async def worker():
        nonlocal count, errors
        while time.time() < end_time:
            try:
                response = await client.get(f"{url}/elevation?lat={lat}&lon={lon}")
                if response.status_code == 200:
                    count += 1
                else:
                    errors += 1
            except httpx.RequestError:
                errors += 1

    start = time.time()
    await asyncio.gather(*[worker() for _ in range(concurrency)])
    actual_duration = time.time() - start

    return ThroughputResults(
        requests_per_second=count / actual_duration if actual_duration > 0 else 0,
        total_requests=count,
        duration_seconds=actual_duration,
        errors=errors,
    )


async def measure_geojson_batch(
    client: httpx.AsyncClient,
    url: str,
    num_points: int,
) -> float:
    """Measure time to process a GeoJSON LineString with N points."""
    # Generate coordinates in a line pattern
    coordinates = [
        [138.0 + i * 0.01, 35.0 + i * 0.01]
        for i in range(num_points)
    ]
    geojson = {"type": "LineString", "coordinates": coordinates}

    start = time.perf_counter()
    response = await client.post(
        f"{url}/elevation",
        json=geojson,
        headers={"Content-Type": "application/json"},
    )
    elapsed = (time.perf_counter() - start) * 1000  # Convert to ms

    if response.status_code != 200:
        console.print(f"[red]GeoJSON batch failed: {response.text}[/red]")
        return -1

    return elapsed


async def warm_cache(
    client: httpx.AsyncClient,
    url: str,
    num_tiles: int,
    start_lat: int = 35,
    start_lon: int = 135,
) -> int:
    """Warm up the cache by querying multiple tiles."""
    loaded = 0
    tiles_per_row = 10

    for i in range(num_tiles):
        lat = start_lat + (i // tiles_per_row) + 0.5
        lon = start_lon + (i % tiles_per_row) + 0.5

        try:
            response = await client.get(f"{url}/elevation?lat={lat}&lon={lon}")
            if response.status_code == 200:
                loaded += 1
        except httpx.RequestError:
            pass

    return loaded


def check_pass(value: float, target: float, lower_is_better: bool = True) -> str:
    """Return pass/fail indicator."""
    if lower_is_better:
        passed = value <= target
    else:
        passed = value >= target

    return "[green]PASS[/green]" if passed else "[red]FAIL[/red]"


async def run_benchmarks(
    url: str,
    container_name: str,
    num_tiles: int = 100,
) -> None:
    """Run the complete benchmark suite."""
    console.print("\n[bold blue]=== HTG Performance Benchmark ===[/bold blue]\n")

    # Check service is running
    async with httpx.AsyncClient(timeout=30.0) as client:
        try:
            response = await client.get(f"{url}/health")
            if response.status_code != 200:
                console.print("[red]Service health check failed![/red]")
                return
            health = response.json()
            console.print(f"Service version: {health.get('version', 'unknown')}\n")
        except httpx.RequestError as e:
            console.print(f"[red]Cannot connect to service: {e}[/red]")
            return

        # --- Memory Benchmark ---
        console.print("[bold]Memory Usage:[/bold]")

        # Baseline memory
        baseline_mem = measure_memory(container_name)
        if baseline_mem:
            console.print(f"  Baseline:     {baseline_mem.current_mb:.1f} MB")
        else:
            console.print("  [yellow]Could not measure memory (is container running?)[/yellow]")

        # Warm cache progressively and measure memory
        for tile_count in [10, 50, 100]:
            if tile_count <= num_tiles:
                loaded = await warm_cache(client, url, tile_count)
                mem = measure_memory(container_name)
                if mem:
                    status = ""
                    if tile_count == 100:
                        status = f" {check_pass(mem.current_mb, TARGETS['memory_100_tiles_mb'])} (target: <{TARGETS['memory_100_tiles_mb']}MB)"
                    console.print(f"  {tile_count} tiles:    {mem.current_mb:.1f} MB{status}")

        console.print()

        # --- Latency Benchmark ---
        console.print("[bold]Latency (1000 requests):[/bold]")

        # Cold start (first request to a new tile - restart would be needed for true cold)
        # Instead, we measure "warm cache" latency since cache is already populated
        warm_latency = await measure_latency(
            client, url, 35.5, 135.5, n_requests=1000
        )
        status = check_pass(warm_latency.p50, TARGETS["latency_cached_ms"])
        console.print(
            f"  Warm cache:   {warm_latency.p50:.2f}ms (p50), "
            f"{warm_latency.p95:.2f}ms (p95), {warm_latency.p99:.2f}ms (p99) "
            f"{status} (target: <{TARGETS['latency_cached_ms']}ms)"
        )

        console.print()

        # --- Throughput Benchmark ---
        console.print("[bold]Throughput:[/bold]")

        throughput = await measure_throughput(
            client, url, 35.5, 135.5, duration_seconds=10, concurrency=10
        )
        status = check_pass(
            throughput.requests_per_second,
            TARGETS["throughput_rps"],
            lower_is_better=False,
        )
        console.print(
            f"  Single tile:  {throughput.requests_per_second:,.0f} req/sec "
            f"{status} (target: >{TARGETS['throughput_rps']})"
        )
        if throughput.errors > 0:
            console.print(f"  [yellow]Errors: {throughput.errors}[/yellow]")

        console.print()

        # --- GeoJSON Batch Benchmark ---
        console.print("[bold]GeoJSON Batch:[/bold]")

        for points in [10, 100, 1000]:
            elapsed = await measure_geojson_batch(client, url, points)
            if elapsed >= 0:
                console.print(f"  {points} points:   {elapsed:.1f}ms")

        console.print()

        # --- Cache Stats ---
        console.print("[bold]Cache Stats:[/bold]")
        try:
            response = await client.get(f"{url}/stats")
            if response.status_code == 200:
                stats = response.json()
                console.print(f"  Cached tiles: {stats.get('cached_tiles', 'N/A')}")
                console.print(f"  Hit rate:     {stats.get('hit_rate', 0) * 100:.1f}%")
        except httpx.RequestError:
            console.print("  [yellow]Could not fetch stats[/yellow]")

        console.print()


def main():
    parser = argparse.ArgumentParser(
        description="HTG Performance Benchmark Suite"
    )
    parser.add_argument(
        "--url",
        default="http://localhost:8080",
        help="Service URL (default: http://localhost:8080)",
    )
    parser.add_argument(
        "--container",
        default="htg-bench",
        help="Docker container name for memory measurement (default: htg-bench)",
    )
    parser.add_argument(
        "--tiles",
        type=int,
        default=100,
        help="Number of tiles to test with (default: 100)",
    )

    args = parser.parse_args()

    asyncio.run(run_benchmarks(args.url, args.container, args.tiles))


if __name__ == "__main__":
    main()
