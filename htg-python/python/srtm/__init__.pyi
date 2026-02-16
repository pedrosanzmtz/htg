"""Type stubs for srtm - High-performance SRTM elevation library."""

from typing import List, Optional, Tuple

__version__: str
VOID_VALUE: int

class CacheStats:
    """Cache statistics for the SRTM service."""

    entry_count: int
    """Number of tiles currently in the cache."""

    hit_count: int
    """Number of cache hits."""

    miss_count: int
    """Number of cache misses."""

    @property
    def hit_rate(self) -> float:
        """Cache hit rate (0.0 to 1.0)."""
        ...

class SrtmService:
    """SRTM elevation service with LRU caching.

    This is the main interface for querying elevation data from SRTM .hgt files.

    Example:
        >>> service = SrtmService("/path/to/srtm", cache_size=100)
        >>> elevation = service.get_elevation(35.3606, 138.7274)
        >>> print(f"Elevation: {elevation}m")
    """

    def __init__(self, data_dir: str, cache_size: int = 100) -> None:
        """Create a new SRTM service.

        Args:
            data_dir: Path to directory containing .hgt files.
            cache_size: Maximum number of tiles to keep in cache.
        """
        ...

    def get_elevation(self, lat: float, lon: float) -> Optional[int]:
        """Get elevation at the specified coordinates using nearest-neighbor lookup.

        Args:
            lat: Latitude in decimal degrees (-60 to 60).
            lon: Longitude in decimal degrees (-180 to 180).

        Returns:
            Elevation in meters, or None if no data available.

        Raises:
            ValueError: If coordinates are out of bounds.
        """
        ...

    def get_elevations_batch(
        self, coords: List[Tuple[float, float]], default: int = 0
    ) -> List[int]:
        """Get elevations for a batch of coordinates.

        Args:
            coords: List of (lat, lon) tuples.
            default: Default value for void/missing data.

        Returns:
            List of elevation values in meters.
        """
        ...

    def get_elevations_batch_interpolated(
        self, coords: List[Tuple[float, float]], default: float = 0.0
    ) -> List[float]:
        """Get interpolated elevations for a batch of coordinates.

        Uses bilinear interpolation for sub-pixel accuracy.

        Args:
            coords: List of (lat, lon) tuples.
            default: Default value for void/missing data.

        Returns:
            List of interpolated elevation values in meters.
        """
        ...

    def get_elevation_interpolated(self, lat: float, lon: float) -> Optional[float]:
        """Get elevation using bilinear interpolation.

        Args:
            lat: Latitude in decimal degrees (-60 to 60).
            lon: Longitude in decimal degrees (-180 to 180).

        Returns:
            Interpolated elevation in meters, or None if any point is void.

        Raises:
            ValueError: If coordinates are out of bounds or tile is not found.
        """
        ...

    def cache_stats(self) -> CacheStats:
        """Get current cache statistics."""
        ...

def lat_lon_to_filename(lat: float, lon: float) -> str:
    """Convert latitude/longitude to SRTM filename.

    Args:
        lat: Latitude in decimal degrees.
        lon: Longitude in decimal degrees.

    Returns:
        Filename string (e.g., "N35E138.hgt").

    Example:
        >>> lat_lon_to_filename(35.5, 138.7)
        'N35E138.hgt'
    """
    ...

def filename_to_lat_lon(filename: str) -> Optional[Tuple[int, int]]:
    """Parse SRTM filename to get base coordinates.

    Args:
        filename: SRTM filename (e.g., "N35E138.hgt").

    Returns:
        Tuple of (latitude, longitude), or None if invalid.

    Example:
        >>> filename_to_lat_lon("N35E138.hgt")
        (35, 138)
    """
    ...
