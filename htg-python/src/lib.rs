//! Python bindings for the htg SRTM elevation library.

#![allow(clippy::useless_conversion)]

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

// Use fully qualified path to avoid collision with the Python module name
use ::htg as htg_lib;

/// Cache statistics for the SRTM service.
#[pyclass]
#[derive(Clone)]
struct CacheStats {
    /// Number of tiles currently in the cache.
    #[pyo3(get)]
    entry_count: u64,
    /// Number of cache hits.
    #[pyo3(get)]
    hit_count: u64,
    /// Number of cache misses.
    #[pyo3(get)]
    miss_count: u64,
}

#[pymethods]
impl CacheStats {
    /// Calculate the cache hit rate (0.0 to 1.0).
    #[getter]
    fn hit_rate(&self) -> f64 {
        let total = self.hit_count + self.miss_count;
        if total == 0 {
            0.0
        } else {
            self.hit_count as f64 / total as f64
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "CacheStats(entry_count={}, hit_count={}, miss_count={}, hit_rate={:.2}%)",
            self.entry_count,
            self.hit_count,
            self.miss_count,
            self.hit_rate() * 100.0
        )
    }
}

/// Statistics from a preload operation.
#[pyclass]
#[derive(Clone)]
struct PreloadStats {
    /// Number of tiles successfully loaded into cache.
    #[pyo3(get)]
    tiles_loaded: u64,
    /// Number of tiles that were already in cache.
    #[pyo3(get)]
    tiles_already_cached: u64,
    /// Number of tiles that failed to load.
    #[pyo3(get)]
    tiles_failed: u64,
    /// Number of tiles that matched the bounding box filter.
    #[pyo3(get)]
    tiles_matched: u64,
    /// Total elapsed time in milliseconds.
    #[pyo3(get)]
    elapsed_ms: u64,
}

#[pymethods]
impl PreloadStats {
    fn __repr__(&self) -> String {
        format!(
            "PreloadStats(loaded={}, cached={}, failed={}, matched={}, elapsed={}ms)",
            self.tiles_loaded,
            self.tiles_already_cached,
            self.tiles_failed,
            self.tiles_matched,
            self.elapsed_ms
        )
    }
}

/// SRTM elevation service with LRU caching.
///
/// This is the main interface for querying elevation data from SRTM .hgt files.
///
/// Example:
///     >>> service = SrtmService("/path/to/srtm", cache_size=100)
///     >>> elevation = service.get_elevation(35.3606, 138.7274)
///     >>> print(f"Elevation: {elevation}m")
#[pyclass]
struct SrtmService {
    inner: Arc<htg_lib::SrtmService>,
}

#[pymethods]
impl SrtmService {
    /// Create a new SRTM service.
    ///
    /// Args:
    ///     data_dir: Path to directory containing .hgt files.
    ///     cache_size: Maximum number of tiles to keep in cache (default: 100).
    ///
    /// Returns:
    ///     A new SrtmService instance.
    #[new]
    #[pyo3(signature = (data_dir, cache_size=100))]
    fn new(data_dir: &str, cache_size: u64) -> Self {
        SrtmService {
            inner: Arc::new(htg_lib::SrtmService::new(data_dir, cache_size)),
        }
    }

    /// Get elevation at the specified coordinates using nearest-neighbor lookup.
    ///
    /// Args:
    ///     lat: Latitude in decimal degrees (-60 to 60).
    ///     lon: Longitude in decimal degrees (-180 to 180).
    ///     rounding: Rounding strategy for grid cell selection.
    ///         "nearest" (default): Round to closest cell (true nearest-neighbor).
    ///         "floor": Always round down (srtm.py compatible, southwest-biased).
    ///
    /// Returns:
    ///     Elevation in meters, or None if no data available (void, missing tile).
    ///
    /// Raises:
    ///     ValueError: If coordinates are out of bounds or rounding is invalid.
    #[pyo3(signature = (lat, lon, rounding="nearest"))]
    fn get_elevation(
        &self,
        py: Python<'_>,
        lat: f64,
        lon: f64,
        rounding: &str,
    ) -> PyResult<Option<i16>> {
        let use_floor = match rounding {
            "nearest" => false,
            "floor" => true,
            _ => {
                return Err(PyValueError::new_err(format!(
                    "Invalid rounding mode '{}'. Use 'nearest' or 'floor'.",
                    rounding
                )))
            }
        };
        let inner = Arc::clone(&self.inner);
        let result = py.allow_threads(move || {
            if use_floor {
                inner.get_elevation_floor(lat, lon)
            } else {
                inner.get_elevation(lat, lon)
            }
        });
        result.map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Get elevations for a batch of coordinates.
    ///
    /// Args:
    ///     coords: List of (lat, lon) tuples.
    ///     default: Default value for void/missing data (default: 0).
    ///     rounding: Rounding strategy for grid cell selection.
    ///         "nearest" (default): Round to closest cell (true nearest-neighbor).
    ///         "floor": Always round down (srtm.py compatible, southwest-biased).
    ///
    /// Returns:
    ///     List of elevation values in meters.
    ///
    /// Raises:
    ///     ValueError: If rounding is invalid.
    #[pyo3(signature = (coords, default=0, rounding="nearest"))]
    fn get_elevations_batch(
        &self,
        py: Python<'_>,
        coords: Vec<(f64, f64)>,
        default: i16,
        rounding: &str,
    ) -> PyResult<Vec<i16>> {
        let use_floor = match rounding {
            "nearest" => false,
            "floor" => true,
            _ => {
                return Err(PyValueError::new_err(format!(
                    "Invalid rounding mode '{}'. Use 'nearest' or 'floor'.",
                    rounding
                )))
            }
        };
        let inner = Arc::clone(&self.inner);
        Ok(py.allow_threads(move || {
            if use_floor {
                inner.get_elevations_batch_floor(&coords, default)
            } else {
                inner.get_elevations_batch(&coords, default)
            }
        }))
    }

    /// Get interpolated elevations for a batch of coordinates.
    ///
    /// Uses bilinear interpolation for sub-pixel accuracy.
    ///
    /// Args:
    ///     coords: List of (lat, lon) tuples.
    ///     default: Default value for void/missing data (default: 0.0).
    ///
    /// Returns:
    ///     List of interpolated elevation values in meters.
    #[pyo3(signature = (coords, default=0.0))]
    fn get_elevations_batch_interpolated(
        &self,
        py: Python<'_>,
        coords: Vec<(f64, f64)>,
        default: f64,
    ) -> Vec<f64> {
        let inner = Arc::clone(&self.inner);
        py.allow_threads(move || inner.get_elevations_batch_interpolated(&coords, default))
    }

    /// Get elevation at the specified coordinates using bilinear interpolation.
    ///
    /// This provides smoother results with sub-pixel accuracy.
    ///
    /// Args:
    ///     lat: Latitude in decimal degrees (-60 to 60).
    ///     lon: Longitude in decimal degrees (-180 to 180).
    ///
    /// Returns:
    ///     Interpolated elevation in meters, or None if any surrounding point is void.
    ///
    /// Raises:
    ///     ValueError: If coordinates are out of bounds or tile is not found.
    fn get_elevation_interpolated(
        &self,
        py: Python<'_>,
        lat: f64,
        lon: f64,
    ) -> PyResult<Option<f64>> {
        let inner = Arc::clone(&self.inner);
        let result = py.allow_threads(move || inner.get_elevation_interpolated(lat, lon));
        result.map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Preload tiles into the LRU cache.
    ///
    /// Scans the data directory for .hgt and .hgt.zip files and loads them
    /// into cache. Useful for warming the cache at startup.
    ///
    /// Args:
    ///     bounds: Optional list of bounding boxes as (min_lat, min_lon, max_lat, max_lon) tuples.
    ///         If provided, only tiles overlapping at least one box are loaded.
    ///     blocking: If True (default), blocks until preload completes and returns stats.
    ///         If False, runs preload in a background thread and returns None immediately.
    ///
    /// Returns:
    ///     PreloadStats if blocking=True, None if blocking=False.
    ///
    /// Example:
    ///     >>> stats = service.preload()
    ///     >>> print(f"Loaded {stats.tiles_loaded} tiles in {stats.elapsed_ms}ms")
    ///
    ///     >>> # Preload only CONUS tiles
    ///     >>> stats = service.preload(bounds=[(24.0, -125.0, 50.0, -66.0)])
    ///
    ///     >>> # Non-blocking preload
    ///     >>> service.preload(blocking=False)
    #[pyo3(signature = (bounds=None, blocking=true))]
    fn preload(
        &self,
        py: Python<'_>,
        bounds: Option<Vec<(f64, f64, f64, f64)>>,
        blocking: bool,
    ) -> Option<PreloadStats> {
        let boxes: Option<Vec<htg_lib::BoundingBox>> = bounds.map(|b| {
            b.into_iter()
                .map(|(min_lat, min_lon, max_lat, max_lon)| {
                    htg_lib::BoundingBox::new(min_lat, min_lon, max_lat, max_lon)
                })
                .collect()
        });

        if blocking {
            let stats = py.allow_threads(|| self.inner.preload(boxes.as_deref()));
            Some(PreloadStats {
                tiles_loaded: stats.tiles_loaded,
                tiles_already_cached: stats.tiles_already_cached,
                tiles_failed: stats.tiles_failed,
                tiles_matched: stats.tiles_matched,
                elapsed_ms: stats.elapsed_ms,
            })
        } else {
            let inner = Arc::clone(&self.inner);
            std::thread::spawn(move || {
                inner.preload(boxes.as_deref());
            });
            None
        }
    }

    /// Get current cache statistics.
    ///
    /// Returns:
    ///     CacheStats object with hit/miss counts and hit rate.
    fn cache_stats(&self) -> CacheStats {
        let stats = self.inner.cache_stats();
        CacheStats {
            entry_count: stats.entry_count,
            hit_count: stats.hit_count,
            miss_count: stats.miss_count,
        }
    }

    fn __repr__(&self) -> String {
        let stats = self.cache_stats();
        format!(
            "SrtmService(cached_tiles={}, hit_rate={:.1}%)",
            stats.entry_count,
            stats.hit_rate() * 100.0
        )
    }
}

/// Convert latitude/longitude to SRTM filename.
///
/// Args:
///     lat: Latitude in decimal degrees.
///     lon: Longitude in decimal degrees.
///
/// Returns:
///     Filename string (e.g., "N35E138.hgt").
///
/// Example:
///     >>> lat_lon_to_filename(35.5, 138.7)
///     'N35E138.hgt'
#[pyfunction]
fn lat_lon_to_filename(lat: f64, lon: f64) -> String {
    htg_lib::filename::lat_lon_to_filename(lat, lon)
}

/// Parse SRTM filename to get base coordinates.
///
/// Args:
///     filename: SRTM filename (e.g., "N35E138.hgt").
///
/// Returns:
///     Tuple of (latitude, longitude) for the tile's southwest corner,
///     or None if the filename is invalid.
///
/// Example:
///     >>> filename_to_lat_lon("N35E138.hgt")
///     (35, 138)
#[pyfunction]
fn filename_to_lat_lon(filename: &str) -> Option<(i32, i32)> {
    htg_lib::filename::filename_to_lat_lon(filename)
}

/// SRTM - High-performance SRTM elevation library.
///
/// This module provides Python bindings for the htg Rust library,
/// enabling fast elevation queries from SRTM .hgt files.
///
/// Example:
///     >>> import srtm
///     >>> service = srtm.SrtmService("/path/to/srtm", cache_size=100)
///     >>> elevation = service.get_elevation(35.3606, 138.7274)
///     >>> print(f"Mount Fuji: {elevation}m")
#[pymodule]
fn srtm(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SrtmService>()?;
    m.add_class::<CacheStats>()?;
    m.add_class::<PreloadStats>()?;
    m.add_function(wrap_pyfunction!(lat_lon_to_filename, m)?)?;
    m.add_function(wrap_pyfunction!(filename_to_lat_lon, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("VOID_VALUE", htg_lib::VOID_VALUE)?;
    Ok(())
}
