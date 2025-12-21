//! Python bindings for the htg SRTM elevation library.

#![allow(clippy::useless_conversion)]

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
    inner: htg_lib::SrtmService,
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
            inner: htg_lib::SrtmService::new(data_dir, cache_size),
        }
    }

    /// Get elevation at the specified coordinates using nearest-neighbor lookup.
    ///
    /// Args:
    ///     lat: Latitude in decimal degrees (-60 to 60).
    ///     lon: Longitude in decimal degrees (-180 to 180).
    ///
    /// Returns:
    ///     Elevation in meters.
    ///
    /// Raises:
    ///     ValueError: If coordinates are out of bounds or tile is not found.
    fn get_elevation(&self, lat: f64, lon: f64) -> PyResult<i16> {
        self.inner
            .get_elevation(lat, lon)
            .map_err(|e| PyValueError::new_err(e.to_string()))
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
    fn get_elevation_interpolated(&self, lat: f64, lon: f64) -> PyResult<Option<f64>> {
        self.inner
            .get_elevation_interpolated(lat, lon)
            .map_err(|e| PyValueError::new_err(e.to_string()))
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
    m.add_function(wrap_pyfunction!(lat_lon_to_filename, m)?)?;
    m.add_function(wrap_pyfunction!(filename_to_lat_lon, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("VOID_VALUE", htg_lib::VOID_VALUE)?;
    Ok(())
}
