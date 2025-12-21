# The native extension module is imported directly by maturin
# This file just ensures the package is recognized and provides type hints
from htg.htg import (  # type: ignore[import]
    CacheStats as CacheStats,
    SrtmService as SrtmService,
    VOID_VALUE as VOID_VALUE,
    __version__ as __version__,
    filename_to_lat_lon as filename_to_lat_lon,
    lat_lon_to_filename as lat_lon_to_filename,
)

__all__ = [
    "SrtmService",
    "CacheStats",
    "lat_lon_to_filename",
    "filename_to_lat_lon",
    "__version__",
    "VOID_VALUE",
]
