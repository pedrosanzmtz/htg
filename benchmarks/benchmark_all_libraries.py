#!/usr/bin/env python3
"""
Comprehensive SRTM Library Comparison

Compares htg-python, srtm.py, and srtm4 performance with LOCAL files only.
htg-python runs FIRST to ensure all .hgt files exist locally.

Usage:
    python benchmark_all_three.py --data-dir ./srtm_cache
"""

import argparse
import sys
import time
from dataclasses import dataclass

try:
    from rich.console import Console
    from rich.table import Table
except ImportError:
    print("Error: Missing rich. Install with: pip install rich")
    sys.exit(1)

console = Console()


@dataclass
class LibraryResult:
    name: str
    implementation: str
    per_query_us: float
    throughput_qps: int
    total_time_ms: float
    available: bool = True
    error: str = ""


def benchmark_htg_python(data_dir: str, n_queries: int = 1000) -> LibraryResult:
    """Benchmark htg-python (Rust + PyO3)."""
    try:
        # Uninstall srtm.py temporarily to avoid namespace conflict
        import subprocess
        subprocess.run(["uv", "pip", "uninstall", "srtm.py"],
                      capture_output=True, check=False)

        # Clear module cache
        if 'srtm' in sys.modules:
            del sys.modules['srtm']

        import srtm

        # Verify it's the right module
        if not hasattr(srtm, 'SrtmService'):
            return LibraryResult(
                "htg-python", "Rust + PyO3", 0, 0, 0, False,
                "Wrong srtm module imported"
            )

        service = srtm.SrtmService(data_dir, cache_size=100)

        # Warmup
        _ = service.get_elevation(35.3606, 138.7274)

        # Benchmark
        start = time.perf_counter()
        for _ in range(n_queries):
            _ = service.get_elevation(35.3606, 138.7274)
        elapsed = time.perf_counter() - start

        return LibraryResult(
            "htg-python",
            "Rust + PyO3",
            (elapsed / n_queries) * 1_000_000,  # microseconds
            int(n_queries / elapsed),
            elapsed * 1000
        )
    except Exception as e:
        return LibraryResult("htg-python", "Rust + PyO3", 0, 0, 0, False, str(e))


def benchmark_srtm_py(data_dir: str, n_queries: int = 1000) -> LibraryResult:
    """Benchmark srtm.py (pure Python)."""
    try:
        # Reinstall srtm.py
        import subprocess
        subprocess.run(["uv", "pip", "install", "srtm.py"],
                      capture_output=True, check=False)

        # Clear module cache
        if 'srtm' in sys.modules:
            del sys.modules['srtm']

        import srtm

        # Verify it's the right module
        if not hasattr(srtm, 'get_data'):
            return LibraryResult(
                "srtm.py", "Pure Python", 0, 0, 0, False,
                "Wrong srtm module imported - htg-python detected"
            )

        elevation_data = srtm.get_data(local_cache_dir=data_dir)

        # Warmup - should use local files created by htg-python
        _ = elevation_data.get_elevation(35.3606, 138.7274)

        # Benchmark
        start = time.perf_counter()
        for _ in range(n_queries):
            _ = elevation_data.get_elevation(35.3606, 138.7274)
        elapsed = time.perf_counter() - start

        return LibraryResult(
            "srtm.py",
            "Pure Python",
            (elapsed / n_queries) * 1_000_000,
            int(n_queries / elapsed),
            elapsed * 1000
        )
    except Exception as e:
        return LibraryResult("srtm.py", "Pure Python", 0, 0, 0, False, str(e))


def benchmark_srtm4(data_dir: str, n_queries: int = 1000) -> LibraryResult:
    """Benchmark srtm4 (Python + C++ subprocess)."""
    try:
        import srtm4

        # Warmup - should use local files created by htg-python
        # Note: srtm4 uses (lon, lat) order
        _ = srtm4.srtm4(138.7274, 35.3606)

        # Benchmark
        start = time.perf_counter()
        for _ in range(n_queries):
            _ = srtm4.srtm4(138.7274, 35.3606)
        elapsed = time.perf_counter() - start

        return LibraryResult(
            "srtm4",
            "Python + C++ (subprocess)",
            (elapsed / n_queries) * 1_000_000,
            int(n_queries / elapsed),
            elapsed * 1000
        )
    except Exception as e:
        return LibraryResult("srtm4", "Python + C++ (subprocess)", 0, 0, 0, False, str(e))


def print_results(results: list[LibraryResult], n_queries: int):
    """Print formatted comparison table."""
    console.print("\n[bold blue]=== Python SRTM Libraries Comparison ===[/bold blue]")
    console.print(f"[cyan]Test:[/cyan] {n_queries:,} queries on LOCAL files (no download)")
    console.print(f"[cyan]Location:[/cyan] Mount Fuji (35.3606°N, 138.7274°E)\n")

    # Main results table
    table = Table(title="Performance Results")
    table.add_column("Library", style="cyan", no_wrap=True)
    table.add_column("Implementation", style="dim")
    table.add_column("Per Query", justify="right", style="yellow")
    table.add_column("Throughput", justify="right", style="green")
    table.add_column("Total Time", justify="right", style="magenta")

    for result in results:
        if not result.available:
            table.add_row(
                result.name,
                result.implementation,
                f"[red]Error: {result.error}[/red]",
                "-",
                "-"
            )
        else:
            table.add_row(
                result.name,
                result.implementation,
                f"{result.per_query_us:.2f} μs",
                f"{result.throughput_qps:,} q/s",
                f"{result.total_time_ms:.2f} ms"
            )

    console.print(table)
    console.print()

    # Comparison table (vs fastest)
    available_results = [r for r in results if r.available]
    if len(available_results) > 1:
        # Find fastest
        fastest = min(available_results, key=lambda r: r.per_query_us)

        comparison_table = Table(title="Relative Performance (vs fastest)")
        comparison_table.add_column("Library", style="cyan")
        comparison_table.add_column("Speed", justify="right", style="yellow")
        comparison_table.add_column("Throughput", justify="right", style="green")

        for result in available_results:
            if result.name == fastest.name:
                comparison_table.add_row(
                    f"[bold]{result.name}[/bold]",
                    "[bold green]1.0x (fastest)[/bold green]",
                    "[bold green]1.0x (highest)[/bold green]"
                )
            else:
                speed_ratio = result.per_query_us / fastest.per_query_us
                throughput_ratio = fastest.throughput_qps / result.throughput_qps
                comparison_table.add_row(
                    result.name,
                    f"{speed_ratio:.1f}x slower",
                    f"{throughput_ratio:.1f}x lower"
                )

        console.print(comparison_table)
        console.print()

        # Summary
        console.print("[bold green]=== Summary ===[/bold green]")
        for result in available_results:
            if result.name != fastest.name:
                speedup = result.per_query_us / fastest.per_query_us
                console.print(
                    f"[green]{fastest.name}[/green] is [yellow]{speedup:.1f}x faster[/yellow] "
                    f"than [cyan]{result.name}[/cyan]"
                )
        console.print()


def main():
    parser = argparse.ArgumentParser(
        description="Compare htg-python, srtm.py, and srtm4 performance"
    )
    parser.add_argument(
        "--data-dir",
        default="./srtm_cache",
        help="Path to SRTM .hgt files directory (default: ./srtm_cache)",
    )
    parser.add_argument(
        "--queries",
        type=int,
        default=1000,
        help="Number of queries per test (default: 1000)",
    )

    args = parser.parse_args()

    console.print("\n[bold yellow]Running benchmarks in order:[/bold yellow]")
    console.print("1. [green]htg-python[/green] (runs FIRST to ensure local files exist)")
    console.print("2. [cyan]srtm.py[/cyan]")
    console.print("3. [yellow]srtm4[/yellow]\n")

    results = []

    # 1. htg-python FIRST (ensures files exist)
    console.print("[bold]1/3:[/bold] Testing htg-python...")
    results.append(benchmark_htg_python(args.data_dir, args.queries))
    if not results[-1].available:
        console.print(f"[red]Error: {results[-1].error}[/red]")
    else:
        console.print(f"[green]✓[/green] {results[-1].throughput_qps:,} queries/sec\n")

    # 2. srtm.py
    console.print("[bold]2/3:[/bold] Testing srtm.py...")
    results.append(benchmark_srtm_py(args.data_dir, args.queries))
    if not results[-1].available:
        console.print(f"[red]Error: {results[-1].error}[/red]")
    else:
        console.print(f"[green]✓[/green] {results[-1].throughput_qps:,} queries/sec\n")

    # 3. srtm4
    console.print("[bold]3/3:[/bold] Testing srtm4...")
    results.append(benchmark_srtm4(args.data_dir, args.queries))
    if not results[-1].available:
        console.print(f"[red]Error: {results[-1].error}[/red]")
    else:
        console.print(f"[green]✓[/green] {results[-1].throughput_qps:,} queries/sec\n")

    # Print comparison
    print_results(results, args.queries)


if __name__ == "__main__":
    main()
