//! SRTM elevation service with LRU caching.
//!
//! This module provides [`SrtmService`], a high-level interface for querying
//! elevation data with automatic tile loading and caching.
//!
//! # Auto-Download Feature
//!
//! When compiled with the `download` feature, `SrtmService` can automatically
//! download missing tiles from a configured data source.
//!
//! ```ignore
//! use htg::{SrtmServiceBuilder, download::DownloadConfig};
//!
//! let service = SrtmServiceBuilder::new("/data/srtm")
//!     .cache_size(100)
//!     .auto_download(DownloadConfig::with_url_template(
//!         "https://example.com/srtm/{filename}.hgt.gz",
//!         true,
//!     ))
//!     .build()?;
//!
//! // Will download N35E138.hgt if not present locally
//! let elevation = service.get_elevation(35.5, 138.5)?;
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use moka::sync::Cache;

use crate::error::{Result, SrtmError};
use crate::filename::lat_lon_to_filename;
use crate::tile::SrtmTile;

#[cfg(feature = "download")]
use crate::download::{DownloadConfig, Downloader};

/// Statistics about cache usage.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of tiles currently in the cache.
    pub entry_count: u64,
    /// Number of cache hits (requests served from cache).
    pub hit_count: u64,
    /// Number of cache misses (tiles loaded from disk).
    pub miss_count: u64,
}

impl CacheStats {
    /// Calculate the cache hit rate (0.0 to 1.0).
    ///
    /// Returns 0.0 if no requests have been made.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hit_count + self.miss_count;
        if total == 0 {
            0.0
        } else {
            self.hit_count as f64 / total as f64
        }
    }
}

/// High-level SRTM elevation service with automatic tile caching.
///
/// `SrtmService` manages loading and caching of SRTM tiles, providing a simple
/// interface to query elevation at any coordinate within the data directory.
///
/// # Example
///
/// ```ignore
/// use htg::SrtmService;
///
/// let service = SrtmService::new("/path/to/hgt/files", 100);
///
/// // Query elevation - tile is loaded automatically
/// let elevation = service.get_elevation(35.6762, 139.6503)?; // Tokyo
/// println!("Elevation: {}m", elevation);
///
/// // Second query in same tile uses cache
/// let elevation2 = service.get_elevation(35.6800, 139.6500)?;
///
/// // Check cache statistics
/// let stats = service.cache_stats();
/// println!("Cache hit rate: {:.1}%", stats.hit_rate() * 100.0);
/// ```
///
/// # Auto-Download (requires `download` feature)
///
/// ```ignore
/// use htg::{SrtmServiceBuilder, download::DownloadConfig};
///
/// let service = SrtmServiceBuilder::new("/data/srtm")
///     .cache_size(100)
///     .auto_download(DownloadConfig::with_url_template(
///         "https://example.com/srtm/{filename}.hgt.gz",
///         true,
///     ))
///     .build()?;
/// ```
pub struct SrtmService {
    /// Directory containing .hgt files.
    data_dir: PathBuf,
    /// LRU cache of loaded tiles.
    tile_cache: Cache<String, Arc<SrtmTile>>,
    /// Number of cache hits.
    hit_count: AtomicU64,
    /// Number of cache misses.
    miss_count: AtomicU64,
    /// Optional downloader for auto-downloading missing tiles.
    #[cfg(feature = "download")]
    downloader: Option<Downloader>,
}

impl SrtmService {
    /// Create a new SRTM service.
    ///
    /// # Arguments
    ///
    /// * `data_dir` - Directory containing `.hgt` files
    /// * `cache_size` - Maximum number of tiles to keep in memory
    ///
    /// # Example
    ///
    /// ```ignore
    /// use htg::SrtmService;
    ///
    /// // Cache up to 100 tiles (~280MB for SRTM3, ~2.5GB for SRTM1)
    /// let service = SrtmService::new("/data/srtm", 100);
    /// ```
    pub fn new<P: AsRef<Path>>(data_dir: P, cache_size: u64) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            tile_cache: Cache::builder().max_capacity(cache_size).build(),
            hit_count: AtomicU64::new(0),
            miss_count: AtomicU64::new(0),
            #[cfg(feature = "download")]
            downloader: None,
        }
    }

    /// Create a builder for more configuration options.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use htg::SrtmService;
    ///
    /// let service = SrtmService::builder("/data/srtm")
    ///     .cache_size(100)
    ///     .build();
    /// ```
    pub fn builder<P: AsRef<Path>>(data_dir: P) -> SrtmServiceBuilder {
        SrtmServiceBuilder::new(data_dir)
    }

    /// Get elevation for the given coordinates using nearest-neighbor lookup.
    ///
    /// This method automatically determines which tile to load, loads it from
    /// disk (or cache), and returns the elevation at the specified location.
    ///
    /// For smoother results with sub-pixel accuracy, use [`get_elevation_interpolated`].
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude in decimal degrees (-60 to 60)
    /// * `lon` - Longitude in decimal degrees (-180 to 180)
    ///
    /// # Returns
    ///
    /// The elevation in meters, or an error if:
    /// - Coordinates are outside SRTM coverage (±60° latitude)
    /// - The required `.hgt` file is not found
    /// - The file is corrupted or has invalid size
    ///
    /// # Example
    ///
    /// ```ignore
    /// let elevation = service.get_elevation(19.4326, -99.1332)?; // Mexico City
    /// println!("Elevation: {}m", elevation);
    /// ```
    pub fn get_elevation(&self, lat: f64, lon: f64) -> Result<i16> {
        let tile = self.load_tile_for_coords(lat, lon)?;
        tile.get_elevation(lat, lon)
    }

    /// Get elevation for the given coordinates using bilinear interpolation.
    ///
    /// This method interpolates between the 4 surrounding grid points for sub-pixel
    /// accuracy. This provides smoother elevation profiles and reduces quantization
    /// error by up to half the grid resolution (~15m for SRTM1, ~45m for SRTM3).
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude in decimal degrees (-60 to 60)
    /// * `lon` - Longitude in decimal degrees (-180 to 180)
    ///
    /// # Returns
    ///
    /// The interpolated elevation in meters as a floating-point value.
    /// Returns `None` if any of the surrounding grid points contains void data.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Interpolated elevation (more accurate)
    /// if let Some(elevation) = service.get_elevation_interpolated(35.6762, 139.6503)? {
    ///     println!("Interpolated elevation: {:.1}m", elevation);
    /// }
    /// ```
    pub fn get_elevation_interpolated(&self, lat: f64, lon: f64) -> Result<Option<f64>> {
        let tile = self.load_tile_for_coords(lat, lon)?;
        tile.get_elevation_interpolated(lat, lon)
    }

    /// Validate coordinates and load the appropriate tile.
    fn load_tile_for_coords(&self, lat: f64, lon: f64) -> Result<Arc<SrtmTile>> {
        // Validate coordinates
        if !(-60.0..=60.0).contains(&lat) {
            return Err(SrtmError::OutOfBounds { lat, lon });
        }
        if !(-180.0..=180.0).contains(&lon) {
            return Err(SrtmError::OutOfBounds { lat, lon });
        }

        // Calculate filename for this coordinate
        let filename = lat_lon_to_filename(lat, lon);

        // Load tile (from cache or disk)
        self.load_tile(&filename)
    }

    /// Load a tile from cache, disk, or download if enabled.
    fn load_tile(&self, filename: &str) -> Result<Arc<SrtmTile>> {
        // Check cache first
        if let Some(tile) = self.tile_cache.get(filename) {
            self.hit_count.fetch_add(1, Ordering::Relaxed);
            return Ok(tile);
        }

        // Cache miss - try to load from disk or download
        self.miss_count.fetch_add(1, Ordering::Relaxed);

        let path = self.data_dir.join(filename);

        // If file doesn't exist, try to download it
        if !path.exists() {
            #[cfg(feature = "download")]
            {
                if let Some(ref downloader) = self.downloader {
                    // Try to download the tile
                    downloader.download_tile_by_name(filename, &self.data_dir)?;
                } else {
                    return Err(SrtmError::TileNotAvailable {
                        filename: filename.to_string(),
                    });
                }
            }

            #[cfg(not(feature = "download"))]
            {
                return Err(SrtmError::FileNotFound { path });
            }
        }

        // Parse base coordinates from filename for the tile
        let (base_lat, base_lon) = crate::filename::filename_to_lat_lon(filename).unwrap_or((0, 0));

        let tile = Arc::new(SrtmTile::from_file_with_coords(&path, base_lat, base_lon)?);

        // Insert into cache
        self.tile_cache.insert(filename.to_string(), tile.clone());

        Ok(tile)
    }

    /// Check if auto-download is enabled.
    #[cfg(feature = "download")]
    pub fn has_auto_download(&self) -> bool {
        self.downloader.is_some()
    }

    /// Get cache statistics.
    ///
    /// Returns information about cache usage including hit rate.
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            entry_count: self.tile_cache.entry_count(),
            hit_count: self.hit_count.load(Ordering::Relaxed),
            miss_count: self.miss_count.load(Ordering::Relaxed),
        }
    }

    /// Get the data directory path.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Get the maximum cache size.
    pub fn cache_capacity(&self) -> u64 {
        self.tile_cache.policy().max_capacity().unwrap_or(0)
    }

    /// Invalidate (remove) a specific tile from the cache.
    ///
    /// This can be useful if you know a tile file has been updated.
    pub fn invalidate_tile(&self, filename: &str) {
        self.tile_cache.invalidate(filename);
    }

    /// Clear all tiles from the cache.
    pub fn clear_cache(&self) {
        self.tile_cache.invalidate_all();
    }
}

/// Builder for creating [`SrtmService`] with custom configuration.
///
/// # Example
///
/// ```ignore
/// use htg::SrtmServiceBuilder;
///
/// let service = SrtmServiceBuilder::new("/data/srtm")
///     .cache_size(100)
///     .build();
/// ```
///
/// # With Auto-Download (requires `download` feature)
///
/// ```ignore
/// use htg::{SrtmServiceBuilder, download::DownloadConfig};
///
/// let service = SrtmServiceBuilder::new("/data/srtm")
///     .cache_size(100)
///     .auto_download(DownloadConfig::with_url_template(
///         "https://example.com/srtm/{filename}.hgt.gz",
///         true,
///     ))
///     .build()?;
/// ```
pub struct SrtmServiceBuilder {
    data_dir: PathBuf,
    cache_size: u64,
    #[cfg(feature = "download")]
    download_config: Option<DownloadConfig>,
}

impl SrtmServiceBuilder {
    /// Create a new builder with the specified data directory.
    pub fn new<P: AsRef<Path>>(data_dir: P) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            cache_size: 100, // Default cache size
            #[cfg(feature = "download")]
            download_config: None,
        }
    }

    /// Create a builder configured from environment variables.
    ///
    /// # Environment Variables
    ///
    /// | Variable | Description | Default |
    /// |----------|-------------|---------|
    /// | `HTG_DATA_DIR` | Directory containing .hgt files | Required |
    /// | `HTG_CACHE_SIZE` | Maximum tiles in cache | 100 |
    /// | `HTG_DOWNLOAD_URL` | URL template for downloads* | None |
    /// | `HTG_DOWNLOAD_GZIP` | Whether URL serves gzip files* | false |
    ///
    /// *Only used when `download` feature is enabled.
    ///
    /// # URL Template Placeholders
    ///
    /// - `{filename}` - Full filename (e.g., "N35E138")
    /// - `{lat_prefix}` - N or S
    /// - `{lat}` - Latitude digits (e.g., "35")
    /// - `{lon_prefix}` - E or W
    /// - `{lon}` - Longitude digits (e.g., "138")
    ///
    /// # Example
    ///
    /// ```bash
    /// export HTG_DATA_DIR=/data/srtm
    /// export HTG_CACHE_SIZE=50
    /// export HTG_DOWNLOAD_URL="https://example.com/srtm/{filename}.hgt.gz"
    /// export HTG_DOWNLOAD_GZIP=true
    /// ```
    ///
    /// ```ignore
    /// use htg::SrtmServiceBuilder;
    ///
    /// let service = SrtmServiceBuilder::from_env()?.build()?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if `HTG_DATA_DIR` is not set.
    pub fn from_env() -> Result<Self> {
        let data_dir = std::env::var("HTG_DATA_DIR").map_err(|_| {
            SrtmError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "HTG_DATA_DIR environment variable not set",
            ))
        })?;

        let cache_size: u64 = std::env::var("HTG_CACHE_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        #[cfg(feature = "download")]
        let download_config = {
            match std::env::var("HTG_DOWNLOAD_URL") {
                Ok(url_template) => {
                    let is_gzipped = std::env::var("HTG_DOWNLOAD_GZIP")
                        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                        .unwrap_or(false);
                    Some(DownloadConfig::with_url_template(url_template, is_gzipped))
                }
                Err(_) => None,
            }
        };

        Ok(Self {
            data_dir: PathBuf::from(data_dir),
            cache_size,
            #[cfg(feature = "download")]
            download_config,
        })
    }

    /// Set the data directory.
    ///
    /// Overrides the directory set in the constructor or from environment.
    pub fn data_dir<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.data_dir = path.as_ref().to_path_buf();
        self
    }

    /// Set the maximum number of tiles to keep in cache.
    ///
    /// Default is 100 tiles.
    pub fn cache_size(mut self, size: u64) -> Self {
        self.cache_size = size;
        self
    }

    /// Enable auto-download with the specified configuration.
    ///
    /// When enabled, missing tiles will be downloaded from the configured source.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use htg::{SrtmServiceBuilder, download::DownloadConfig};
    ///
    /// let service = SrtmServiceBuilder::new("/data/srtm")
    ///     .auto_download(DownloadConfig::with_url_template(
    ///         "https://example.com/{filename}.hgt.gz",
    ///         true, // is gzipped
    ///     ))
    ///     .build()?;
    /// ```
    #[cfg(feature = "download")]
    pub fn auto_download(mut self, config: DownloadConfig) -> Self {
        self.download_config = Some(config);
        self
    }

    /// Build the [`SrtmService`].
    ///
    /// # Errors
    ///
    /// Returns an error if auto-download is enabled but the downloader
    /// cannot be created (e.g., due to TLS initialization failure).
    #[cfg(feature = "download")]
    pub fn build(self) -> Result<SrtmService> {
        let downloader = match self.download_config {
            Some(config) => Some(Downloader::new(config)?),
            None => None,
        };

        Ok(SrtmService {
            data_dir: self.data_dir,
            tile_cache: Cache::builder().max_capacity(self.cache_size).build(),
            hit_count: AtomicU64::new(0),
            miss_count: AtomicU64::new(0),
            downloader,
        })
    }

    /// Build the [`SrtmService`].
    #[cfg(not(feature = "download"))]
    pub fn build(self) -> SrtmService {
        SrtmService {
            data_dir: self.data_dir,
            tile_cache: Cache::builder().max_capacity(self.cache_size).build(),
            hit_count: AtomicU64::new(0),
            miss_count: AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    /// File size for SRTM3 (1201 × 1201 × 2 bytes)
    const SRTM3_SIZE: usize = 1201 * 1201 * 2;
    const SRTM3_SAMPLES: usize = 1201;

    /// Create a test SRTM3 file with elevation = 500m at center
    fn create_test_tile(dir: &Path, filename: &str, center_elevation: i16) {
        let mut data = vec![0u8; SRTM3_SIZE];

        // Set center elevation (row 600, col 600)
        let center_offset = (600 * SRTM3_SAMPLES + 600) * 2;
        let bytes = center_elevation.to_be_bytes();
        data[center_offset] = bytes[0];
        data[center_offset + 1] = bytes[1];

        let path = dir.join(filename);
        let mut file = fs::File::create(path).unwrap();
        file.write_all(&data).unwrap();
    }

    #[test]
    fn test_service_basic() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        // Query center of tile
        let elevation = service.get_elevation(35.5, 138.5).unwrap();
        assert_eq!(elevation, 500);
    }

    #[test]
    fn test_cache_hit() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        // First query - cache miss
        let _ = service.get_elevation(35.5, 138.5).unwrap();
        let stats1 = service.cache_stats();
        assert_eq!(stats1.miss_count, 1);
        assert_eq!(stats1.hit_count, 0);

        // Second query in same tile - cache hit
        let _ = service.get_elevation(35.6, 138.6).unwrap();
        let stats2 = service.cache_stats();
        assert_eq!(stats2.miss_count, 1);
        assert_eq!(stats2.hit_count, 1);

        // Note: entry_count may be lazy, so we just verify hit/miss counts
    }

    #[test]
    fn test_multiple_tiles() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);
        create_test_tile(temp_dir.path(), "N36E138.hgt", 1000);

        let service = SrtmService::new(temp_dir.path(), 10);

        let elev1 = service.get_elevation(35.5, 138.5).unwrap();
        let elev2 = service.get_elevation(36.5, 138.5).unwrap();

        assert_eq!(elev1, 500);
        assert_eq!(elev2, 1000);

        let stats = service.cache_stats();
        // Verify miss count (entry_count may be lazy)
        assert_eq!(stats.miss_count, 2);
    }

    #[test]
    fn test_invalid_coordinates() {
        let temp_dir = TempDir::new().unwrap();
        let service = SrtmService::new(temp_dir.path(), 10);

        // Latitude out of SRTM coverage
        assert!(service.get_elevation(70.0, 0.0).is_err());
        assert!(service.get_elevation(-70.0, 0.0).is_err());

        // Longitude out of range
        assert!(service.get_elevation(0.0, 200.0).is_err());
        assert!(service.get_elevation(0.0, -200.0).is_err());
    }

    #[test]
    fn test_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let service = SrtmService::new(temp_dir.path(), 10);

        // Query for a tile that doesn't exist
        let result = service.get_elevation(50.0, 50.0);
        assert!(result.is_err());

        // Error type depends on whether download feature is enabled
        #[cfg(not(feature = "download"))]
        {
            if let Err(SrtmError::FileNotFound { path }) = result {
                assert!(path.to_string_lossy().contains("N50E050.hgt"));
            } else {
                panic!("Expected FileNotFound error");
            }
        }

        #[cfg(feature = "download")]
        {
            if let Err(SrtmError::TileNotAvailable { filename }) = result {
                assert!(filename.contains("N50E050"));
            } else {
                panic!("Expected TileNotAvailable error");
            }
        }
    }

    #[test]
    fn test_cache_stats() {
        let stats = CacheStats {
            entry_count: 5,
            hit_count: 80,
            miss_count: 20,
        };

        assert_eq!(stats.hit_rate(), 0.8);

        let empty_stats = CacheStats::default();
        assert_eq!(empty_stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_clear_cache() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        // Load a tile
        let _ = service.get_elevation(35.5, 138.5).unwrap();
        assert_eq!(service.cache_stats().miss_count, 1);

        // Clear cache
        service.clear_cache();

        // After clearing, next access should be a miss again
        let _ = service.get_elevation(35.5, 138.5).unwrap();
        assert_eq!(service.cache_stats().miss_count, 2);
    }

    #[test]
    fn test_cache_capacity() {
        let temp_dir = TempDir::new().unwrap();
        let service = SrtmService::new(temp_dir.path(), 100);

        assert_eq!(service.cache_capacity(), 100);
    }

    #[test]
    fn test_from_env_missing_data_dir() {
        // Temporarily unset the env var if it exists
        let original = std::env::var("HTG_DATA_DIR").ok();
        std::env::remove_var("HTG_DATA_DIR");

        let result = SrtmServiceBuilder::from_env();
        assert!(result.is_err());

        // Restore original value if it existed
        if let Some(val) = original {
            std::env::set_var("HTG_DATA_DIR", val);
        }
    }

    #[test]
    fn test_from_env_with_values() {
        let temp_dir = TempDir::new().unwrap();

        // Save original values
        let orig_dir = std::env::var("HTG_DATA_DIR").ok();
        let orig_size = std::env::var("HTG_CACHE_SIZE").ok();

        // Set test values
        std::env::set_var("HTG_DATA_DIR", temp_dir.path());
        std::env::set_var("HTG_CACHE_SIZE", "50");

        let builder = SrtmServiceBuilder::from_env().unwrap();
        assert_eq!(builder.data_dir, temp_dir.path());
        assert_eq!(builder.cache_size, 50);

        // Restore original values
        match orig_dir {
            Some(v) => std::env::set_var("HTG_DATA_DIR", v),
            None => std::env::remove_var("HTG_DATA_DIR"),
        }
        match orig_size {
            Some(v) => std::env::set_var("HTG_CACHE_SIZE", v),
            None => std::env::remove_var("HTG_CACHE_SIZE"),
        }
    }

    #[test]
    fn test_from_env_default_cache_size() {
        let temp_dir = TempDir::new().unwrap();

        // Save original values
        let orig_dir = std::env::var("HTG_DATA_DIR").ok();
        let orig_size = std::env::var("HTG_CACHE_SIZE").ok();

        // Set only data dir, no cache size
        std::env::set_var("HTG_DATA_DIR", temp_dir.path());
        std::env::remove_var("HTG_CACHE_SIZE");

        let builder = SrtmServiceBuilder::from_env().unwrap();
        assert_eq!(builder.cache_size, 100); // Default value

        // Restore original values
        match orig_dir {
            Some(v) => std::env::set_var("HTG_DATA_DIR", v),
            None => std::env::remove_var("HTG_DATA_DIR"),
        }
        match orig_size {
            Some(v) => std::env::set_var("HTG_CACHE_SIZE", v),
            None => std::env::remove_var("HTG_CACHE_SIZE"),
        }
    }
}
