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
//!         "https://example.com/srtm/{filename}.hgt.gz", // compression auto-detected
//!     ))
//!     .build()?;
//!
//! // Will download N35E138.hgt if not present locally
//! let elevation = service.get_elevation(35.5, 138.5)?;
//! ```

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use moka::sync::Cache;

use crate::error::{Result, SrtmError};
use crate::filename::{coords_to_filename, filename_to_lat_lon};
use crate::tile::{SrtmTile, VOID_VALUE};

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

/// A geographic bounding box for filtering tiles during preload.
///
/// Coordinates are in decimal degrees (WGS84).
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    /// Minimum latitude (southern boundary).
    pub min_lat: f64,
    /// Minimum longitude (western boundary).
    pub min_lon: f64,
    /// Maximum latitude (northern boundary).
    pub max_lat: f64,
    /// Maximum longitude (eastern boundary).
    pub max_lon: f64,
}

impl BoundingBox {
    /// Create a new bounding box.
    ///
    /// # Arguments
    ///
    /// * `min_lat` - Southern boundary latitude
    /// * `min_lon` - Western boundary longitude
    /// * `max_lat` - Northern boundary latitude
    /// * `max_lon` - Eastern boundary longitude
    pub fn new(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> Self {
        Self {
            min_lat,
            min_lon,
            max_lat,
            max_lon,
        }
    }

    /// Check if this bounding box overlaps with a 1°×1° tile.
    ///
    /// A tile at `(tile_lat, tile_lon)` covers the area
    /// `[tile_lat, tile_lat+1) × [tile_lon, tile_lon+1)`.
    pub fn overlaps_tile(&self, tile_lat: i32, tile_lon: i32) -> bool {
        let tile_max_lat = tile_lat + 1;
        let tile_max_lon = tile_lon + 1;

        self.min_lat < tile_max_lat as f64
            && self.max_lat > tile_lat as f64
            && self.min_lon < tile_max_lon as f64
            && self.max_lon > tile_lon as f64
    }
}

/// Statistics from a preload operation.
#[derive(Debug, Clone, Default)]
pub struct PreloadStats {
    /// Number of tiles successfully loaded into cache.
    pub tiles_loaded: u64,
    /// Number of tiles that were already in cache.
    pub tiles_already_cached: u64,
    /// Number of tiles that failed to load.
    pub tiles_failed: u64,
    /// Number of tiles that matched the bounding box filter.
    pub tiles_matched: u64,
    /// Total elapsed time in milliseconds.
    pub elapsed_ms: u64,
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
///         "https://example.com/srtm/{filename}.hgt.gz", // compression auto-detected
///     ))
///     .build()?;
/// ```
pub struct SrtmService {
    /// Directory containing .hgt files.
    data_dir: PathBuf,
    /// LRU cache of loaded tiles, keyed by (floor_lat, floor_lon).
    tile_cache: Cache<(i32, i32), Arc<SrtmTile>>,
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
    /// For smoother results with sub-pixel accuracy, use [`Self::get_elevation_interpolated`].
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude in decimal degrees (-60 to 60)
    /// * `lon` - Longitude in decimal degrees (-180 to 180)
    ///
    /// # Returns
    ///
    /// - `Ok(Some(elevation))` - elevation in meters
    /// - `Ok(None)` - void data, missing tile, or tile not available
    /// - `Err(...)` - coordinates out of bounds, corrupted file, or I/O error
    ///
    /// # Example
    ///
    /// ```ignore
    /// let elevation = service.get_elevation(19.4326, -99.1332)?; // Mexico City
    /// if let Some(elev) = elevation {
    ///     println!("Elevation: {}m", elev);
    /// }
    /// ```
    pub fn get_elevation(&self, lat: f64, lon: f64) -> Result<Option<i16>> {
        match self.load_tile_for_coords(lat, lon) {
            Ok(tile) => {
                let v = tile.get_elevation(lat, lon)?;
                Ok(if v == VOID_VALUE { None } else { Some(v) })
            }
            Err(SrtmError::FileNotFound { .. }) | Err(SrtmError::TileNotAvailable { .. }) => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Get elevation using floor-based rounding (srtm.py compatible).
    ///
    /// This method uses `floor()` instead of `round()` for grid cell selection,
    /// producing results compatible with srtm.py. The difference is typically
    /// 1-3 meters for coordinates near grid cell boundaries.
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude in decimal degrees (-60 to 60)
    /// * `lon` - Longitude in decimal degrees (-180 to 180)
    ///
    /// # Returns
    ///
    /// - `Ok(Some(elevation))` - elevation in meters
    /// - `Ok(None)` - void data, missing tile, or tile not available
    /// - `Err(...)` - coordinates out of bounds, corrupted file, or I/O error
    pub fn get_elevation_floor(&self, lat: f64, lon: f64) -> Result<Option<i16>> {
        match self.load_tile_for_coords(lat, lon) {
            Ok(tile) => {
                let v = tile.get_elevation_floor(lat, lon)?;
                Ok(if v == VOID_VALUE { None } else { Some(v) })
            }
            Err(SrtmError::FileNotFound { .. }) | Err(SrtmError::TileNotAvailable { .. }) => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
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
        match self.load_tile_for_coords(lat, lon) {
            Ok(tile) => tile.get_elevation_interpolated(lat, lon),
            Err(SrtmError::FileNotFound { .. }) | Err(SrtmError::TileNotAvailable { .. }) => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Get elevations for a batch of coordinates.
    ///
    /// Coordinates are grouped by tile so that each unique tile is loaded only
    /// once, regardless of how many coordinates fall within it.
    ///
    /// Returns a vector of elevation values, one per input coordinate.
    /// Uses `default` for void data, missing tiles, or errors.
    ///
    /// # Arguments
    ///
    /// * `coords` - Slice of (latitude, longitude) pairs
    /// * `default` - Default value for void/missing/error results
    ///
    /// # Example
    ///
    /// ```ignore
    /// let coords = vec![(35.3606, 138.7274), (27.9881, 86.9250)];
    /// let elevations = service.get_elevations_batch(&coords, 0);
    /// ```
    pub fn get_elevations_batch(&self, coords: &[(f64, f64)], default: i16) -> Vec<i16> {
        self.batch_with_tile_grouping(coords, default, |tile, lat, lon| {
            match tile.get_elevation(lat, lon) {
                Ok(v) if v != VOID_VALUE => Some(v),
                _ => None,
            }
        })
    }

    /// Get elevations for a batch of coordinates using floor-based rounding.
    ///
    /// Coordinates are grouped by tile so that each unique tile is loaded only
    /// once, regardless of how many coordinates fall within it.
    ///
    /// Returns a vector of elevation values, one per input coordinate.
    /// Uses floor-based rounding for srtm.py compatibility.
    /// Uses `default` for void data, missing tiles, or errors.
    ///
    /// # Arguments
    ///
    /// * `coords` - Slice of (latitude, longitude) pairs
    /// * `default` - Default value for void/missing/error results
    pub fn get_elevations_batch_floor(&self, coords: &[(f64, f64)], default: i16) -> Vec<i16> {
        self.batch_with_tile_grouping(coords, default, |tile, lat, lon| {
            match tile.get_elevation_floor(lat, lon) {
                Ok(v) if v != VOID_VALUE => Some(v),
                _ => None,
            }
        })
    }

    /// Get interpolated elevations for a batch of coordinates.
    ///
    /// Coordinates are grouped by tile so that each unique tile is loaded only
    /// once, regardless of how many coordinates fall within it.
    ///
    /// Returns a vector of interpolated elevation values, one per input coordinate.
    /// Uses bilinear interpolation for sub-pixel accuracy.
    /// Uses `default` for void data, missing tiles, or errors.
    ///
    /// # Arguments
    ///
    /// * `coords` - Slice of (latitude, longitude) pairs
    /// * `default` - Default value for void/missing/error results
    ///
    /// # Example
    ///
    /// ```ignore
    /// let coords = vec![(35.3606, 138.7274), (27.9881, 86.9250)];
    /// let elevations = service.get_elevations_batch_interpolated(&coords, 0.0);
    /// ```
    pub fn get_elevations_batch_interpolated(
        &self,
        coords: &[(f64, f64)],
        default: f64,
    ) -> Vec<f64> {
        self.batch_with_tile_grouping(coords, default, |tile, lat, lon| {
            tile.get_elevation_interpolated(lat, lon).ok().flatten()
        })
    }

    /// Generic tile-grouped batch helper.
    ///
    /// Groups coordinates by tile key, loads each unique tile once, applies
    /// the elevation function, and reassembles results in original input order.
    fn batch_with_tile_grouping<T: Copy>(
        &self,
        coords: &[(f64, f64)],
        default: T,
        elevation_fn: impl Fn(&SrtmTile, f64, f64) -> Option<T>,
    ) -> Vec<T> {
        let mut results = vec![default; coords.len()];

        // Group coordinate indices by tile key
        let mut groups: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, &(lat, lon)) in coords.iter().enumerate() {
            // Out-of-bounds coords get the default (skip grouping)
            if !(-60.0..=60.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                continue;
            }
            let key = (lat.floor() as i32, lon.floor() as i32);
            groups.entry(key).or_default().push(i);
        }

        // Process each tile group: 1 cache lookup per tile, not per coord
        for (key, indices) in &groups {
            let tile = match self.load_tile(*key) {
                Ok(t) => t,
                Err(_) => continue, // missing tile → all coords get default
            };

            for &i in indices {
                let (lat, lon) = coords[i];
                if let Some(v) = elevation_fn(&tile, lat, lon) {
                    results[i] = v;
                }
            }
        }

        results
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

        // Compute tile key directly — no heap allocation
        let key = (lat.floor() as i32, lon.floor() as i32);

        // Load tile (from cache or disk)
        self.load_tile(key)
    }

    /// Load a tile from cache, disk, or download if enabled.
    fn load_tile(&self, key: (i32, i32)) -> Result<Arc<SrtmTile>> {
        // Check cache first — no heap allocation for the key
        if let Some(tile) = self.tile_cache.get(&key) {
            self.hit_count.fetch_add(1, Ordering::Relaxed);
            return Ok(tile);
        }

        // Cache miss - generate filename string only now
        self.miss_count.fetch_add(1, Ordering::Relaxed);

        let filename = coords_to_filename(key.0, key.1);
        let path = self.data_dir.join(&filename);

        // If file doesn't exist, try zip extraction or download
        if !path.exists() {
            // Check for local .hgt.zip file
            let zip_path = self.data_dir.join(format!("{}.zip", filename));
            if zip_path.exists() {
                self.extract_hgt_from_zip(&zip_path, &filename)?;
            } else {
                #[cfg(feature = "download")]
                {
                    if let Some(ref downloader) = self.downloader {
                        // Try to download the tile
                        downloader.download_tile_by_name(&filename, &self.data_dir)?;
                    } else {
                        return Err(SrtmError::TileNotAvailable { filename });
                    }
                }

                #[cfg(not(feature = "download"))]
                {
                    return Err(SrtmError::FileNotFound { path });
                }
            }
        }

        let tile = Arc::new(SrtmTile::from_file_with_coords(&path, key.0, key.1)?);

        // Insert into cache
        self.tile_cache.insert(key, tile.clone());

        Ok(tile)
    }

    /// Extract an .hgt file from a local .hgt.zip archive.
    fn extract_hgt_from_zip(&self, zip_path: &Path, filename: &str) -> Result<()> {
        let file = std::fs::File::open(zip_path).map_err(SrtmError::Io)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| SrtmError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

        // Look for the .hgt file inside the archive
        let mut found = false;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| {
                SrtmError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;

            let entry_name = entry.name().to_string();
            if entry_name.ends_with(".hgt") || entry_name == filename {
                let out_path = self.data_dir.join(filename);
                let mut out_file = std::fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut out_file)?;
                found = true;
                break;
            }
        }

        if !found {
            return Err(SrtmError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No .hgt file found in {}", zip_path.display()),
            )));
        }

        Ok(())
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
    /// Accepts a filename (e.g., "N35E138.hgt") and parses the coordinates
    /// to find the cache entry.
    pub fn invalidate_tile(&self, filename: &str) {
        if let Some(key) = filename_to_lat_lon(filename) {
            self.tile_cache.invalidate(&key);
        }
    }

    /// Clear all tiles from the cache.
    pub fn clear_cache(&self) {
        self.tile_cache.invalidate_all();
    }

    /// Scan the data directory for `.hgt` and `.hgt.zip` files.
    ///
    /// Returns a sorted, deduplicated list of tile filenames (e.g., `["N35E138.hgt"]`).
    /// Both `.hgt` and `.hgt.zip` files are discovered; duplicates are merged
    /// (if both `N35E138.hgt` and `N35E138.hgt.zip` exist, only `N35E138.hgt` appears once).
    pub fn scan_tile_files(&self) -> Vec<String> {
        let mut filenames = HashSet::new();

        let entries = match std::fs::read_dir(&self.data_dir) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if name.ends_with(".hgt.zip") {
                // Strip .zip suffix to get the canonical .hgt name
                let hgt_name = name.strip_suffix(".zip").unwrap();
                filenames.insert(hgt_name.to_string());
            } else if name.ends_with(".hgt") {
                filenames.insert(name.to_string());
            }
        }

        let mut result: Vec<String> = filenames.into_iter().collect();
        result.sort();
        result
    }

    /// Preload tiles into the LRU cache.
    ///
    /// Scans the data directory for `.hgt` and `.hgt.zip` files and loads them
    /// into the cache. Optionally filters tiles by one or more bounding boxes.
    ///
    /// This is useful for warming the cache at startup to avoid cold-start latency
    /// when tiles are stored on high-latency storage (e.g., NFS).
    ///
    /// # Arguments
    ///
    /// * `bounds` - Optional slice of bounding boxes to filter tiles. If `None`,
    ///   all discovered tiles are loaded. If `Some`, only tiles that overlap with
    ///   at least one bounding box are loaded.
    ///
    /// # Returns
    ///
    /// Statistics about the preload operation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use htg::{SrtmService, BoundingBox};
    ///
    /// let service = SrtmService::new("/data/srtm", 100);
    ///
    /// // Preload all tiles
    /// let stats = service.preload(None);
    /// println!("Loaded {} tiles in {}ms", stats.tiles_loaded, stats.elapsed_ms);
    ///
    /// // Preload only CONUS tiles
    /// let conus = BoundingBox::new(24.0, -125.0, 50.0, -66.0);
    /// let stats = service.preload(Some(&[conus]));
    /// ```
    pub fn preload(&self, bounds: Option<&[BoundingBox]>) -> PreloadStats {
        let start = Instant::now();
        let mut stats = PreloadStats::default();

        let filenames = self.scan_tile_files();

        for filename in &filenames {
            // Parse coordinates from filename to get tile key
            let key = match filename_to_lat_lon(filename) {
                Some(k) => k,
                None => continue, // Can't parse coordinates, skip
            };

            // Filter by bounding boxes if provided
            if let Some(boxes) = bounds {
                if !boxes.iter().any(|b| b.overlaps_tile(key.0, key.1)) {
                    continue;
                }
            }

            stats.tiles_matched += 1;

            // Check if already in cache
            if self.tile_cache.get(&key).is_some() {
                stats.tiles_already_cached += 1;
                continue;
            }

            // Load the tile
            match self.load_tile(key) {
                Ok(_) => stats.tiles_loaded += 1,
                Err(_) => stats.tiles_failed += 1,
            }
        }

        stats.elapsed_ms = start.elapsed().as_millis() as u64;
        stats
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
///         "https://example.com/srtm/{filename}.hgt.gz", // compression auto-detected
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
    /// | `HTG_DOWNLOAD_SOURCE` | Named source: "ardupilot"* | None |
    /// | `HTG_DOWNLOAD_URL` | URL template for custom downloads* | None |
    /// | `HTG_DOWNLOAD_GZIP` | Whether URL serves gzip files* | false |
    ///
    /// *Only used when `download` feature is enabled.
    ///
    /// # Named Sources
    ///
    /// - `ardupilot` or `ardupilot-srtm1` - ArduPilot SRTM1 (30m resolution, ~25MB/tile)
    /// - `ardupilot-srtm3` - ArduPilot SRTM3 (90m resolution, ~2.8MB/tile)
    ///
    /// # URL Template Placeholders
    ///
    /// - `{filename}` - Full filename (e.g., "N35E138")
    /// - `{lat_prefix}` - N or S
    /// - `{lat}` - Latitude digits (e.g., "35")
    /// - `{lon_prefix}` - E or W
    /// - `{lon}` - Longitude digits (e.g., "138")
    /// - `{continent}` - Continent subdirectory (e.g., "Eurasia", "North_America")
    ///
    /// # Example
    ///
    /// ```bash
    /// # Using ArduPilot source (recommended)
    /// export HTG_DATA_DIR=/data/srtm
    /// export HTG_DOWNLOAD_SOURCE=ardupilot
    ///
    /// # Or using custom URL template
    /// export HTG_DATA_DIR=/data/srtm
    /// export HTG_CACHE_SIZE=50
    /// export HTG_DOWNLOAD_URL="https://example.com/srtm/{filename}.hgt.gz"
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
            // Check for named source first (e.g., "ardupilot")
            if let Ok(source) = std::env::var("HTG_DOWNLOAD_SOURCE") {
                match source.to_lowercase().as_str() {
                    "ardupilot" | "ardupilot-srtm1" => Some(DownloadConfig::ardupilot_srtm1()),
                    "ardupilot-srtm3" => Some(DownloadConfig::ardupilot_srtm3()),
                    _ => {
                        // Unknown source name, fall through to URL template
                        None
                    }
                }
            } else {
                None
            }
            .or_else(|| {
                // Fall back to custom URL template
                match std::env::var("HTG_DOWNLOAD_URL") {
                    Ok(url_template) => {
                        // Check for explicit compression setting, otherwise auto-detect from URL
                        if let Ok(gzip_setting) = std::env::var("HTG_DOWNLOAD_GZIP") {
                            let is_gzipped =
                                gzip_setting.eq_ignore_ascii_case("true") || gzip_setting == "1";
                            let compression = if is_gzipped {
                                crate::download::Compression::Gzip
                            } else {
                                crate::download::Compression::None
                            };
                            Some(DownloadConfig::with_url_template_and_compression(
                                url_template,
                                compression,
                            ))
                        } else {
                            Some(DownloadConfig::with_url_template(url_template))
                        }
                    }
                    Err(_) => None,
                }
            })
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
    /// Compression format is auto-detected from the URL extension (.gz, .zip).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use htg::{SrtmServiceBuilder, download::DownloadConfig};
    ///
    /// let service = SrtmServiceBuilder::new("/data/srtm")
    ///     .auto_download(DownloadConfig::with_url_template(
    ///         "https://example.com/{filename}.hgt.gz", // compression auto-detected
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
        assert_eq!(elevation, Some(500));
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

        assert_eq!(elev1, Some(500));
        assert_eq!(elev2, Some(1000));

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

        // Query for a tile that doesn't exist — returns Ok(None)
        let result = service.get_elevation(50.0, 50.0).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_missing_file_interpolated() {
        let temp_dir = TempDir::new().unwrap();
        let service = SrtmService::new(temp_dir.path(), 10);

        // Query for a tile that doesn't exist — returns Ok(None)
        let result = service.get_elevation_interpolated(50.0, 50.0).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_void_data_returns_none() {
        let temp_dir = TempDir::new().unwrap();
        // Create tile where center elevation is VOID_VALUE
        create_test_tile(temp_dir.path(), "N35E138.hgt", VOID_VALUE);

        let service = SrtmService::new(temp_dir.path(), 10);

        // Void data returns None
        let result = service.get_elevation(35.5, 138.5).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_elevations_batch() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        let coords = vec![
            (35.5, 138.5), // valid tile, center = 500
            (50.0, 50.0),  // missing tile
            (35.1, 138.1), // valid tile, edge = 0 (default data)
        ];
        let results = service.get_elevations_batch(&coords, -1);

        assert_eq!(results[0], 500);
        assert_eq!(results[1], -1); // missing tile → default
                                    // Third result is 0 (zero-filled tile data), which is a valid value
        assert_eq!(results[2], 0);
    }

    #[test]
    fn test_get_elevations_batch_interpolated() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        let coords = vec![
            (35.5, 138.5), // valid tile, center = 500.0
            (50.0, 50.0),  // missing tile
        ];
        let results = service.get_elevations_batch_interpolated(&coords, -1.0);

        assert_eq!(results.len(), 2);
        assert!((results[0] - 500.0).abs() < 1.0); // interpolated, close to 500
        assert_eq!(results[1], -1.0); // missing tile → default
    }

    #[test]
    fn test_hgt_zip_extraction() {
        let temp_dir = TempDir::new().unwrap();

        // Create a .hgt file and zip it
        let hgt_data = vec![0u8; SRTM3_SIZE];
        let zip_path = temp_dir.path().join("N40E010.hgt.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip_writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip_writer.start_file("N40E010.hgt", options).unwrap();
        zip_writer.write_all(&hgt_data).unwrap();
        zip_writer.finish().unwrap();

        let service = SrtmService::new(temp_dir.path(), 10);

        // Query should extract from zip and return elevation
        let result = service.get_elevation(40.5, 10.5).unwrap();
        assert_eq!(result, Some(0)); // zero-filled data

        // Extracted .hgt file should now exist
        assert!(temp_dir.path().join("N40E010.hgt").exists());
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
    fn test_get_elevation_floor() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        // At center, floor and normal should agree
        let elev = service.get_elevation_floor(35.5, 138.5).unwrap();
        assert_eq!(elev, Some(500));
    }

    #[test]
    fn test_get_elevation_floor_missing_tile() {
        let temp_dir = TempDir::new().unwrap();
        let service = SrtmService::new(temp_dir.path(), 10);

        let result = service.get_elevation_floor(50.0, 50.0).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_elevations_batch_floor() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        let coords = vec![
            (35.5, 138.5), // valid tile, center = 500
            (50.0, 50.0),  // missing tile
        ];
        let results = service.get_elevations_batch_floor(&coords, -1);

        assert_eq!(results[0], 500);
        assert_eq!(results[1], -1); // missing tile → default
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

    // --- Preload tests ---

    #[test]
    fn test_preload_all_tiles() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);
        create_test_tile(temp_dir.path(), "N36E139.hgt", 1000);

        let service = SrtmService::new(temp_dir.path(), 10);
        let stats = service.preload(None);

        assert_eq!(stats.tiles_matched, 2);
        assert_eq!(stats.tiles_loaded, 2);
        assert_eq!(stats.tiles_already_cached, 0);
        assert_eq!(stats.tiles_failed, 0);
    }

    #[test]
    fn test_preload_with_bounding_box() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);
        create_test_tile(temp_dir.path(), "N50E010.hgt", 1000);

        let service = SrtmService::new(temp_dir.path(), 10);

        // Bounding box that only covers Japan area (N35E138)
        let bbox = BoundingBox::new(34.0, 137.0, 37.0, 140.0);
        let stats = service.preload(Some(&[bbox]));

        assert_eq!(stats.tiles_matched, 1);
        assert_eq!(stats.tiles_loaded, 1);

        // Verify the right tile was loaded
        let elev = service.get_elevation(35.5, 138.5).unwrap();
        assert_eq!(elev, Some(500));
    }

    #[test]
    fn test_preload_multiple_bounding_boxes() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);
        create_test_tile(temp_dir.path(), "N50E010.hgt", 1000);
        create_test_tile(temp_dir.path(), "N20E020.hgt", 200);

        let service = SrtmService::new(temp_dir.path(), 10);

        // Two bounding boxes: one for Japan, one for Europe
        let japan = BoundingBox::new(34.0, 137.0, 37.0, 140.0);
        let europe = BoundingBox::new(49.0, 9.0, 52.0, 12.0);
        let stats = service.preload(Some(&[japan, europe]));

        assert_eq!(stats.tiles_matched, 2); // N35E138 and N50E010
        assert_eq!(stats.tiles_loaded, 2);
    }

    #[test]
    fn test_preload_already_cached() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);
        create_test_tile(temp_dir.path(), "N36E139.hgt", 1000);

        let service = SrtmService::new(temp_dir.path(), 10);

        // First preload
        let stats1 = service.preload(None);
        assert_eq!(stats1.tiles_loaded, 2);
        assert_eq!(stats1.tiles_already_cached, 0);

        // Second preload — tiles should be cached
        let stats2 = service.preload(None);
        assert_eq!(stats2.tiles_loaded, 0);
        assert_eq!(stats2.tiles_already_cached, 2);
    }

    #[test]
    fn test_preload_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let service = SrtmService::new(temp_dir.path(), 10);

        let stats = service.preload(None);
        assert_eq!(stats.tiles_matched, 0);
        assert_eq!(stats.tiles_loaded, 0);
    }

    #[test]
    fn test_preload_with_zip_files() {
        let temp_dir = TempDir::new().unwrap();

        // Create a .hgt.zip file
        let hgt_data = vec![0u8; SRTM3_SIZE];
        let zip_path = temp_dir.path().join("N40E010.hgt.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip_writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip_writer.start_file("N40E010.hgt", options).unwrap();
        zip_writer.write_all(&hgt_data).unwrap();
        zip_writer.finish().unwrap();

        // Also create a regular .hgt file
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);
        let stats = service.preload(None);

        assert_eq!(stats.tiles_matched, 2);
        assert_eq!(stats.tiles_loaded, 2);
        assert_eq!(stats.tiles_failed, 0);
    }

    #[test]
    fn test_bounding_box_overlaps_tile() {
        // Tile N35E138 covers [35, 36) x [138, 139)
        let bbox = BoundingBox::new(35.5, 138.5, 36.5, 139.5);
        assert!(bbox.overlaps_tile(35, 138)); // overlaps

        // Completely outside
        let bbox = BoundingBox::new(40.0, 140.0, 41.0, 141.0);
        assert!(!bbox.overlaps_tile(35, 138)); // no overlap

        // Touching edge (exclusive boundary)
        let bbox = BoundingBox::new(36.0, 139.0, 37.0, 140.0);
        assert!(!bbox.overlaps_tile(35, 138)); // tile ends at 36,139

        // Bbox fully contains tile
        let bbox = BoundingBox::new(34.0, 137.0, 37.0, 140.0);
        assert!(bbox.overlaps_tile(35, 138));

        // Tile fully contains bbox
        let bbox = BoundingBox::new(35.2, 138.2, 35.8, 138.8);
        assert!(bbox.overlaps_tile(35, 138));

        // Negative coordinates
        let bbox = BoundingBox::new(-13.5, -78.5, -11.5, -76.5);
        assert!(bbox.overlaps_tile(-13, -78));
        assert!(bbox.overlaps_tile(-12, -78));
        assert!(bbox.overlaps_tile(-13, -77));
    }

    #[test]
    fn test_preload_no_match() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        // Bounding box far from any tile
        let bbox = BoundingBox::new(-50.0, -50.0, -49.0, -49.0);
        let stats = service.preload(Some(&[bbox]));

        assert_eq!(stats.tiles_matched, 0);
        assert_eq!(stats.tiles_loaded, 0);
    }

    #[test]
    fn test_scan_tile_files() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);
        create_test_tile(temp_dir.path(), "N36E139.hgt", 1000);

        // Create a non-tile file that should be ignored
        fs::write(temp_dir.path().join("readme.txt"), "not a tile").unwrap();

        let service = SrtmService::new(temp_dir.path(), 10);
        let files = service.scan_tile_files();

        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "N35E138.hgt");
        assert_eq!(files[1], "N36E139.hgt");
    }

    #[test]
    fn test_scan_tile_files_deduplicates_zip() {
        let temp_dir = TempDir::new().unwrap();

        // Create both .hgt and .hgt.zip for same tile
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);
        let hgt_data = vec![0u8; SRTM3_SIZE];
        let zip_path = temp_dir.path().join("N35E138.hgt.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip_writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip_writer.start_file("N35E138.hgt", options).unwrap();
        zip_writer.write_all(&hgt_data).unwrap();
        zip_writer.finish().unwrap();

        let service = SrtmService::new(temp_dir.path(), 10);
        let files = service.scan_tile_files();

        // Should deduplicate to single entry
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], "N35E138.hgt");
    }
}
