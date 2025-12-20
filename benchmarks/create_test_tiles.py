#!/usr/bin/env python3
"""
Generate synthetic SRTM tiles for benchmarking.

Creates .hgt files in the SRTM format:
- Big-endian signed 16-bit integers
- Row-major order (north to south, west to east)
- SRTM3: 1201x1201 samples (~2.8MB)
- SRTM1: 3601x3601 samples (~25MB)
"""

import argparse
import struct
from pathlib import Path

# File sizes
SRTM3_SAMPLES = 1201
SRTM3_SIZE = SRTM3_SAMPLES * SRTM3_SAMPLES * 2  # 2,884,802 bytes

SRTM1_SAMPLES = 3601
SRTM1_SIZE = SRTM1_SAMPLES * SRTM1_SAMPLES * 2  # 25,934,402 bytes


def create_srtm_tile(
    path: Path,
    samples: int,
    base_elevation: int = 500,
    pattern: str = "gradient",
) -> None:
    """
    Create a synthetic SRTM tile file.

    Args:
        path: Output file path
        samples: Grid size (1201 for SRTM3, 3601 for SRTM1)
        base_elevation: Base elevation value
        pattern: Elevation pattern - "gradient", "flat", or "random"
    """
    data = bytearray(samples * samples * 2)

    for row in range(samples):
        for col in range(samples):
            if pattern == "gradient":
                # Create a gradient pattern for visual verification
                elevation = base_elevation + (row + col) % 1000
            elif pattern == "flat":
                elevation = base_elevation
            else:
                # Simple deterministic "random" pattern
                elevation = base_elevation + ((row * 31 + col * 17) % 500)

            offset = (row * samples + col) * 2
            data[offset : offset + 2] = struct.pack(">h", elevation)

    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def lat_lon_to_filename(lat: int, lon: int) -> str:
    """Convert lat/lon to SRTM filename format."""
    lat_prefix = "N" if lat >= 0 else "S"
    lon_prefix = "E" if lon >= 0 else "W"
    return f"{lat_prefix}{abs(lat):02d}{lon_prefix}{abs(lon):03d}.hgt"


def create_test_tiles(
    output_dir: Path,
    num_tiles: int = 100,
    resolution: str = "srtm3",
    start_lat: int = 35,
    start_lon: int = 135,
) -> list[Path]:
    """
    Create multiple test tiles for benchmarking.

    Args:
        output_dir: Directory to create tiles in
        num_tiles: Number of tiles to create
        resolution: "srtm3" (2.8MB) or "srtm1" (25MB)
        start_lat: Starting latitude
        start_lon: Starting longitude

    Returns:
        List of created tile paths
    """
    samples = SRTM3_SAMPLES if resolution == "srtm3" else SRTM1_SAMPLES
    created = []

    # Create tiles in a grid pattern
    tiles_per_row = 10
    for i in range(num_tiles):
        lat = start_lat + (i // tiles_per_row)
        lon = start_lon + (i % tiles_per_row)

        # Wrap around if we go too far
        if lat > 60:
            lat = start_lat
        if lon > 180:
            lon = start_lon

        filename = lat_lon_to_filename(lat, lon)
        path = output_dir / filename

        # Use different base elevations for variety
        base_elevation = 100 + (i * 50) % 2000

        create_srtm_tile(path, samples, base_elevation, pattern="gradient")
        created.append(path)

    return created


def main():
    parser = argparse.ArgumentParser(
        description="Generate synthetic SRTM tiles for benchmarking"
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("benchmarks/test_tiles"),
        help="Output directory for tiles (default: benchmarks/test_tiles)",
    )
    parser.add_argument(
        "--num-tiles",
        type=int,
        default=100,
        help="Number of tiles to create (default: 100)",
    )
    parser.add_argument(
        "--resolution",
        choices=["srtm3", "srtm1", "both"],
        default="srtm3",
        help="Tile resolution (default: srtm3)",
    )
    parser.add_argument(
        "--start-lat",
        type=int,
        default=35,
        help="Starting latitude (default: 35)",
    )
    parser.add_argument(
        "--start-lon",
        type=int,
        default=135,
        help="Starting longitude (default: 135)",
    )

    args = parser.parse_args()

    print(f"Creating {args.num_tiles} test tiles in {args.output_dir}")

    if args.resolution in ("srtm3", "both"):
        srtm3_dir = args.output_dir / "srtm3"
        tiles = create_test_tiles(
            srtm3_dir,
            args.num_tiles,
            "srtm3",
            args.start_lat,
            args.start_lon,
        )
        total_size = len(tiles) * SRTM3_SIZE / (1024 * 1024)
        print(f"Created {len(tiles)} SRTM3 tiles ({total_size:.1f} MB)")

    if args.resolution in ("srtm1", "both"):
        srtm1_dir = args.output_dir / "srtm1"
        tiles = create_test_tiles(
            srtm1_dir,
            args.num_tiles,
            "srtm1",
            args.start_lat,
            args.start_lon,
        )
        total_size = len(tiles) * SRTM1_SIZE / (1024 * 1024)
        print(f"Created {len(tiles)} SRTM1 tiles ({total_size:.1f} MB)")

    print("Done!")


if __name__ == "__main__":
    main()
