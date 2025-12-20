#!/usr/bin/env python3
"""
HTG Elevation Comparison Tool

Compares htg-service elevation results against popular external APIs
(OpenTopoData, Open-Elevation) to validate accuracy.

Usage:
    # Start htg-service first
    export HTG_DATA_DIR=/path/to/srtm
    export HTG_DOWNLOAD_URL="https://terrain.ardupilot.org/SRTM3/{filename}"
    cargo run --release -p htg-service

    # Run comparison
    cd scripts
    uv run python compare_elevations.py
"""

import json
import time
from pathlib import Path

import httpx
from rich.console import Console
from rich.table import Table

# API endpoints
HTG_URL = "http://localhost:8080/elevation"
OPENTOPODATA_URL = "https://api.opentopodata.org/v1/srtm90m"
OPENELEVATION_URL = "https://api.open-elevation.com/api/v1/lookup"

# Cache file for API responses
CACHE_FILE = Path(__file__).parent / "api_cache.json"

# Test locations covering diverse global regions
TEST_LOCATIONS = [
    {"name": "Mount Fuji", "lat": 35.3606, "lon": 138.7274, "desc": "Japan's highest peak"},
    {"name": "Death Valley", "lat": 36.2308, "lon": -116.7677, "desc": "Below sea level"},
    {"name": "Denver", "lat": 39.7392, "lon": -104.9903, "desc": "Mile High City"},
    {"name": "Tokyo", "lat": 35.6762, "lon": 139.6503, "desc": "Coastal city"},
    {"name": "Cape Town", "lat": -33.9249, "lon": 18.4241, "desc": "Southern hemisphere"},
    {"name": "Amazon Basin", "lat": -3.1190, "lon": -60.0217, "desc": "Tropical lowland"},
    {"name": "Swiss Alps", "lat": 46.5197, "lon": 7.5597, "desc": "Steep terrain"},
    {"name": "La Paz", "lat": -16.5000, "lon": -68.1500, "desc": "High altitude city"},
    {"name": "Grand Canyon", "lat": 36.0544, "lon": -112.1401, "desc": "Dramatic terrain"},
    {"name": "Lhasa", "lat": 29.6500, "lon": 91.1000, "desc": "Tibetan Plateau"},
]


def load_cache() -> dict:
    """Load cached API responses from file."""
    if CACHE_FILE.exists():
        try:
            return json.loads(CACHE_FILE.read_text())
        except json.JSONDecodeError:
            return {}
    return {}


def save_cache(cache: dict) -> None:
    """Save API responses to cache file."""
    CACHE_FILE.write_text(json.dumps(cache, indent=2))


def query_htg(client: httpx.Client, lat: float, lon: float) -> float | None:
    """Query local htg-service with interpolation enabled."""
    try:
        resp = client.get(
            HTG_URL,
            params={"lat": lat, "lon": lon, "interpolate": "true"},
            timeout=30.0,
        )
        if resp.status_code == 200:
            return resp.json()["elevation"]
        return None
    except httpx.RequestError:
        return None


def query_opentopodata(
    client: httpx.Client, lat: float, lon: float, cache: dict
) -> float | None:
    """Query OpenTopoData API with caching and rate limiting."""
    key = f"otd:{lat},{lon}"
    if key in cache:
        return cache[key]

    # Rate limit: 1 request per second
    time.sleep(1.1)

    try:
        resp = client.get(
            OPENTOPODATA_URL,
            params={"locations": f"{lat},{lon}"},
            timeout=30.0,
        )
        if resp.status_code == 200:
            data = resp.json()
            if data.get("results") and data["results"][0].get("elevation") is not None:
                elev = data["results"][0]["elevation"]
                cache[key] = elev
                return elev
        return None
    except httpx.RequestError:
        return None


def query_openelevation(
    client: httpx.Client, lat: float, lon: float, cache: dict
) -> float | None:
    """Query Open-Elevation API with caching and rate limiting."""
    key = f"oe:{lat},{lon}"
    if key in cache:
        return cache[key]

    # Conservative rate limit
    time.sleep(0.5)

    try:
        resp = client.get(
            OPENELEVATION_URL,
            params={"locations": f"{lat},{lon}"},
            timeout=30.0,
        )
        if resp.status_code == 200:
            data = resp.json()
            if data.get("results") and data["results"][0].get("elevation") is not None:
                elev = data["results"][0]["elevation"]
                cache[key] = elev
                return elev
        return None
    except httpx.RequestError:
        return None


def calculate_stats(differences: list[float]) -> dict:
    """Calculate summary statistics for differences."""
    if not differences:
        return {}

    n = len(differences)
    abs_diffs = [abs(d) for d in differences]
    mean = sum(abs_diffs) / n
    max_diff = max(abs_diffs)
    variance = sum((d - mean) ** 2 for d in abs_diffs) / n
    std_dev = variance**0.5
    within_1m = sum(1 for d in abs_diffs if d <= 1) / n * 100
    within_5m = sum(1 for d in abs_diffs if d <= 5) / n * 100

    return {
        "mean": mean,
        "max": max_diff,
        "std_dev": std_dev,
        "within_1m": within_1m,
        "within_5m": within_5m,
    }


def main() -> None:
    """Run elevation comparison against external APIs."""
    console = Console()
    cache = load_cache()

    console.print("\n[bold]HTG Elevation Comparison Tool[/bold]\n")
    console.print("Comparing htg-service (interpolated) against external APIs:\n")
    console.print("  - OpenTopoData (SRTM 90m)")
    console.print("  - Open-Elevation\n")

    # Build results table
    table = Table(title="Elevation Comparison (meters)")
    table.add_column("Location", style="cyan", no_wrap=True)
    table.add_column("Lat", justify="right")
    table.add_column("Lon", justify="right")
    table.add_column("HTG", justify="right", style="green")
    table.add_column("OTD", justify="right")
    table.add_column("OE", justify="right")
    table.add_column("Diff(OTD)", justify="right")
    table.add_column("Diff(OE)", justify="right")

    otd_diffs: list[float] = []
    oe_diffs: list[float] = []

    with httpx.Client() as client:
        for loc in TEST_LOCATIONS:
            console.print(f"  Querying {loc['name']}...", end="\r")

            htg = query_htg(client, loc["lat"], loc["lon"])
            otd = query_opentopodata(client, loc["lat"], loc["lon"], cache)
            oe = query_openelevation(client, loc["lat"], loc["lon"], cache)

            # Calculate differences
            if htg is not None and otd is not None:
                otd_diff = htg - otd
                otd_diffs.append(otd_diff)
                otd_diff_str = f"{otd_diff:+.1f}"
            else:
                otd_diff_str = "N/A"

            if htg is not None and oe is not None:
                oe_diff = htg - oe
                oe_diffs.append(oe_diff)
                oe_diff_str = f"{oe_diff:+.1f}"
            else:
                oe_diff_str = "N/A"

            table.add_row(
                loc["name"],
                f"{loc['lat']:.4f}",
                f"{loc['lon']:.4f}",
                f"{htg:.1f}" if htg is not None else "N/A",
                f"{otd:.1f}" if otd is not None else "N/A",
                f"{oe:.1f}" if oe is not None else "N/A",
                otd_diff_str,
                oe_diff_str,
            )

    # Save cache for future runs
    save_cache(cache)

    # Print results
    console.print(" " * 40, end="\r")  # Clear progress line
    console.print(table)

    # Print statistics
    console.print("\n[bold]Summary Statistics[/bold]\n")

    if otd_diffs:
        stats = calculate_stats(otd_diffs)
        console.print("[cyan]OpenTopoData vs HTG:[/cyan]")
        console.print(f"  Mean absolute error: {stats['mean']:.1f}m")
        console.print(f"  Max error: {stats['max']:.1f}m")
        console.print(f"  Std deviation: {stats['std_dev']:.1f}m")
        console.print(f"  Within +/-1m: {stats['within_1m']:.0f}%")
        console.print(f"  Within +/-5m: {stats['within_5m']:.0f}%")
    else:
        console.print("[yellow]No OpenTopoData comparisons available[/yellow]")

    if oe_diffs:
        stats = calculate_stats(oe_diffs)
        console.print("\n[cyan]Open-Elevation vs HTG:[/cyan]")
        console.print(f"  Mean absolute error: {stats['mean']:.1f}m")
        console.print(f"  Max error: {stats['max']:.1f}m")
        console.print(f"  Std deviation: {stats['std_dev']:.1f}m")
        console.print(f"  Within +/-1m: {stats['within_1m']:.0f}%")
        console.print(f"  Within +/-5m: {stats['within_5m']:.0f}%")
    else:
        console.print("\n[yellow]No Open-Elevation comparisons available[/yellow]")

    # Overall verdict
    console.print()
    if otd_diffs or oe_diffs:
        all_diffs = otd_diffs + oe_diffs
        max_error = max(abs(d) for d in all_diffs) if all_diffs else 0
        if max_error <= 5:
            console.print("[green bold]Result: PASS[/green bold] - All elevations within 5m tolerance")
        elif max_error <= 30:
            console.print("[yellow bold]Result: WARNING[/yellow bold] - Some differences up to {:.1f}m".format(max_error))
        else:
            console.print("[red bold]Result: FAIL[/red bold] - Differences exceed 30m threshold")
    else:
        console.print("[red]Result: ERROR[/red] - No comparisons could be made. Is htg-service running?")


if __name__ == "__main__":
    main()
