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

class PreloadStats:
    """Statistics from a preload operation."""

    tiles_loaded: int
    """Number of tiles successfully loaded into cache."""

    tiles_already_cached: int
    """Number of tiles that were already in cache."""

    tiles_failed: int
    """Number of tiles that failed to load."""

    tiles_matched: int
    """Number of tiles that matched the bounding box filter."""

    elapsed_ms: int
    """Total elapsed time in milliseconds."""

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

    def get_elevation(
        self, lat: float, lon: float, rounding: str = "nearest"
    ) -> Optional[int]:
        """Get elevation at the specified coordinates using nearest-neighbor lookup.

        Args:
            lat: Latitude in decimal degrees (-60 to 60).
            lon: Longitude in decimal degrees (-180 to 180).
            rounding: Rounding strategy for grid cell selection.
                "nearest" (default): Round to closest cell (true nearest-neighbor).
                "floor": Always round down (srtm.py compatible, southwest-biased).

        Returns:
            Elevation in meters, or None if no data available.

        Raises:
            ValueError: If coordinates are out of bounds or rounding is invalid.
        """
        ...

    def get_elevations_batch(
        self,
        coords: List[Tuple[float, float]],
        default: int = 0,
        rounding: str = "nearest",
    ) -> List[int]:
        """Get elevations for a batch of coordinates.

        Args:
            coords: List of (lat, lon) tuples.
            default: Default value for void/missing data.
            rounding: Rounding strategy for grid cell selection.
                "nearest" (default): Round to closest cell (true nearest-neighbor).
                "floor": Always round down (srtm.py compatible, southwest-biased).

        Returns:
            List of elevation values in meters.

        Raises:
            ValueError: If rounding is invalid.
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

    def preload(
        self,
        bounds: Optional[List[Tuple[float, float, float, float]]] = None,
        blocking: bool = True,
    ) -> Optional[PreloadStats]:
        """Preload tiles into the LRU cache.

        Scans the data directory for .hgt and .hgt.zip files and loads them
        into cache. Useful for warming the cache at startup.

        Args:
            bounds: Optional list of bounding boxes as (min_lat, min_lon, max_lat, max_lon) tuples.
                If provided, only tiles overlapping at least one box are loaded.
            blocking: If True (default), blocks until preload completes and returns stats.
                If False, runs preload in a background thread and returns None immediately.

        Returns:
            PreloadStats if blocking=True, None if blocking=False.
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
