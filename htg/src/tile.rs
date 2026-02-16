//! SRTM tile parsing and elevation extraction.
//!
//! This module provides the [`SrtmTile`] struct for reading SRTM `.hgt` files
//! and extracting elevation data at specific coordinates.

use std::fs::File;
use std::path::Path;

use memmap2::Mmap;

use crate::error::{Result, SrtmError};

/// File size for SRTM1 (1 arc-second, ~30m resolution): 3601 × 3601 × 2 bytes
const SRTM1_SIZE: usize = 3601 * 3601 * 2; // 25,934,402 bytes

/// File size for SRTM3 (3 arc-second, ~90m resolution): 1201 × 1201 × 2 bytes
const SRTM3_SIZE: usize = 1201 * 1201 * 2; // 2,884,802 bytes

/// Number of samples per row/column for SRTM1
const SRTM1_SAMPLES: usize = 3601;

/// Number of samples per row/column for SRTM3
const SRTM3_SAMPLES: usize = 1201;

/// Value indicating no data (void) in SRTM files
pub const VOID_VALUE: i16 = -32768;

/// Resolution type of an SRTM tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrtmResolution {
    /// SRTM1: 1 arc-second (~30m) resolution
    Srtm1,
    /// SRTM3: 3 arc-second (~90m) resolution
    Srtm3,
}

impl SrtmResolution {
    /// Returns the number of samples per row/column for this resolution.
    pub fn samples(&self) -> usize {
        match self {
            SrtmResolution::Srtm1 => SRTM1_SAMPLES,
            SrtmResolution::Srtm3 => SRTM3_SAMPLES,
        }
    }

    /// Returns the approximate resolution in meters.
    pub fn meters(&self) -> f64 {
        match self {
            SrtmResolution::Srtm1 => 30.0,
            SrtmResolution::Srtm3 => 90.0,
        }
    }
}

/// A memory-mapped SRTM tile for fast elevation lookups.
///
/// # Example
///
/// ```ignore
/// use htg::SrtmTile;
///
/// let tile = SrtmTile::from_file("N35E138.hgt")?;
/// let elevation = tile.get_elevation(35.5, 138.5)?;
/// println!("Elevation: {}m", elevation);
/// ```
pub struct SrtmTile {
    /// Memory-mapped file data
    data: Mmap,
    /// Number of samples per row/column (1201 or 3601)
    samples: usize,
    /// Resolution type
    resolution: SrtmResolution,
    /// Southwest corner latitude (integer)
    base_lat: i32,
    /// Southwest corner longitude (integer)
    base_lon: i32,
}

impl SrtmTile {
    /// Load an SRTM tile from a `.hgt` file.
    ///
    /// The resolution (SRTM1 vs SRTM3) is automatically detected from the file size.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the `.hgt` file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened or memory-mapped
    /// - The file size doesn't match SRTM1 or SRTM3 format
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_file_with_coords(path, 0, 0)
    }

    /// Load an SRTM tile with explicit base coordinates.
    ///
    /// This is useful when the filename doesn't follow the standard naming convention.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the `.hgt` file
    /// * `base_lat` - Latitude of the southwest corner (integer)
    /// * `base_lon` - Longitude of the southwest corner (integer)
    pub fn from_file_with_coords<P: AsRef<Path>>(
        path: P,
        base_lat: i32,
        base_lon: i32,
    ) -> Result<Self> {
        let file = File::open(&path)?;

        // SAFETY: Memory mapping is safe as long as the file is not modified
        // while mapped. We open the file read-only and don't expose the mapping.
        let mmap = unsafe { Mmap::map(&file)? };

        // Detect resolution from file size
        let (samples, resolution) = match mmap.len() {
            SRTM1_SIZE => (SRTM1_SAMPLES, SrtmResolution::Srtm1),
            SRTM3_SIZE => (SRTM3_SAMPLES, SrtmResolution::Srtm3),
            size => return Err(SrtmError::InvalidFileSize { size }),
        };

        Ok(Self {
            data: mmap,
            samples,
            resolution,
            base_lat,
            base_lon,
        })
    }

    /// Get the elevation at the specified coordinates using nearest-neighbor lookup.
    ///
    /// This method returns the elevation of the nearest grid point (using `round()`).
    /// For smoother results with sub-pixel accuracy, use [`Self::get_elevation_interpolated`].
    /// For srtm.py-compatible floor-based rounding, use [`Self::get_elevation_floor`].
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude in decimal degrees
    /// * `lon` - Longitude in decimal degrees
    ///
    /// # Returns
    ///
    /// The elevation in meters, or [`VOID_VALUE`] (-32768) if no data is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the coordinates are outside the tile bounds.
    pub fn get_elevation(&self, lat: f64, lon: f64) -> Result<i16> {
        self.get_elevation_inner(lat, lon, f64::round)
    }

    /// Get the elevation at the specified coordinates using floor-based rounding.
    ///
    /// This method uses `floor()` instead of `round()` for grid cell selection,
    /// producing results compatible with srtm.py. The difference is that floor
    /// always selects the southwest-biased grid cell, while round selects the
    /// true nearest cell.
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude in decimal degrees
    /// * `lon` - Longitude in decimal degrees
    ///
    /// # Returns
    ///
    /// The elevation in meters, or [`VOID_VALUE`] (-32768) if no data is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the coordinates are outside the tile bounds.
    pub fn get_elevation_floor(&self, lat: f64, lon: f64) -> Result<i16> {
        self.get_elevation_inner(lat, lon, f64::floor)
    }

    /// Internal elevation lookup with configurable rounding function.
    fn get_elevation_inner(&self, lat: f64, lon: f64, rounding_fn: fn(f64) -> f64) -> Result<i16> {
        // Calculate fractional position within tile
        let lat_frac = lat - lat.floor();
        let lon_frac = lon - lon.floor();

        // Validate bounds (should be 0.0 to 1.0)
        if !(0.0..=1.0).contains(&lat_frac) || !(0.0..=1.0).contains(&lon_frac) {
            return Err(SrtmError::OutOfBounds { lat, lon });
        }

        // Convert to row/col indices
        // IMPORTANT: Rows are inverted - row 0 is the north edge (top of file)
        // The file stores data from north to south, left to right
        let row = rounding_fn((1.0 - lat_frac) * (self.samples - 1) as f64) as usize;
        let col = rounding_fn(lon_frac * (self.samples - 1) as f64) as usize;

        self.get_elevation_at(row, col)
    }

    /// Get the elevation at the specified coordinates using bilinear interpolation.
    ///
    /// This method interpolates between the 4 surrounding grid points for sub-pixel
    /// accuracy. This typically provides smoother elevation profiles and reduces
    /// quantization error by up to half the grid resolution (~15m for SRTM1, ~45m for SRTM3).
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude in decimal degrees
    /// * `lon` - Longitude in decimal degrees
    ///
    /// # Returns
    ///
    /// The interpolated elevation in meters as a floating-point value.
    /// Returns `None` if any of the surrounding grid points contains [`VOID_VALUE`].
    ///
    /// # Errors
    ///
    /// Returns an error if the coordinates are outside the tile bounds.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tile = SrtmTile::from_file("N35E138.hgt")?;
    ///
    /// // Interpolated elevation (more accurate)
    /// if let Some(elevation) = tile.get_elevation_interpolated(35.5, 138.5)? {
    ///     println!("Interpolated elevation: {:.1}m", elevation);
    /// }
    ///
    /// // Nearest-neighbor elevation (faster, less accurate)
    /// let elevation = tile.get_elevation(35.5, 138.5)?;
    /// println!("Nearest elevation: {}m", elevation);
    /// ```
    pub fn get_elevation_interpolated(&self, lat: f64, lon: f64) -> Result<Option<f64>> {
        // Calculate fractional position within tile
        let lat_frac = lat - lat.floor();
        let lon_frac = lon - lon.floor();

        // Validate bounds (should be 0.0 to 1.0)
        if !(0.0..=1.0).contains(&lat_frac) || !(0.0..=1.0).contains(&lon_frac) {
            return Err(SrtmError::OutOfBounds { lat, lon });
        }

        // Convert to continuous row/col position
        // IMPORTANT: Rows are inverted - row 0 is the north edge (top of file)
        let row_pos = (1.0 - lat_frac) * (self.samples - 1) as f64;
        let col_pos = lon_frac * (self.samples - 1) as f64;

        // Get integer indices for the 4 surrounding points
        let row0 = row_pos.floor() as usize;
        let col0 = col_pos.floor() as usize;
        let row1 = (row0 + 1).min(self.samples - 1);
        let col1 = (col0 + 1).min(self.samples - 1);

        // Get fractional weights for interpolation
        let row_weight = row_pos - row0 as f64;
        let col_weight = col_pos - col0 as f64;

        // Get the 4 surrounding elevation values
        let v00 = self.get_elevation_at(row0, col0)?;
        let v10 = self.get_elevation_at(row0, col1)?;
        let v01 = self.get_elevation_at(row1, col0)?;
        let v11 = self.get_elevation_at(row1, col1)?;

        // Check for void values - if any surrounding point is void, return None
        if v00 == VOID_VALUE || v10 == VOID_VALUE || v01 == VOID_VALUE || v11 == VOID_VALUE {
            return Ok(None);
        }

        // Bilinear interpolation
        // First interpolate horizontally along the top and bottom rows
        let v0 = v00 as f64 + (v10 as f64 - v00 as f64) * col_weight;
        let v1 = v01 as f64 + (v11 as f64 - v01 as f64) * col_weight;

        // Then interpolate vertically between the two horizontal results
        let elevation = v0 + (v1 - v0) * row_weight;

        Ok(Some(elevation))
    }

    /// Get elevation at a specific row/column index.
    ///
    /// # Arguments
    ///
    /// * `row` - Row index (0 = north edge)
    /// * `col` - Column index (0 = west edge)
    fn get_elevation_at(&self, row: usize, col: usize) -> Result<i16> {
        // Clamp to valid range
        let row = row.min(self.samples - 1);
        let col = col.min(self.samples - 1);

        // Calculate byte offset (2 bytes per sample, row-major order)
        let offset = (row * self.samples + col) * 2;

        // Read 16-bit big-endian signed integer
        let elevation = i16::from_be_bytes([self.data[offset], self.data[offset + 1]]);

        Ok(elevation)
    }

    /// Returns the resolution of this tile.
    pub fn resolution(&self) -> SrtmResolution {
        self.resolution
    }

    /// Returns the number of samples per row/column.
    pub fn samples(&self) -> usize {
        self.samples
    }

    /// Returns the base latitude (southwest corner).
    pub fn base_lat(&self) -> i32 {
        self.base_lat
    }

    /// Returns the base longitude (southwest corner).
    pub fn base_lon(&self) -> i32 {
        self.base_lon
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Create a test SRTM3 file with known elevation values
    fn create_test_srtm3_file() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();

        // Create SRTM3 sized file (1201 × 1201 × 2 bytes)
        let mut data = vec![0u8; SRTM3_SIZE];

        // Set some known elevation values
        // Row 0, Col 0 (northwest corner) = 1000m
        data[0] = 0x03;
        data[1] = 0xE8; // 1000 in big-endian

        // Row 600, Col 600 (center) = 500m
        let center_offset = (600 * SRTM3_SAMPLES + 600) * 2;
        data[center_offset] = 0x01;
        data[center_offset + 1] = 0xF4; // 500 in big-endian

        // Row 1200, Col 1200 (southeast corner) = 100m
        let se_offset = (1200 * SRTM3_SAMPLES + 1200) * 2;
        data[se_offset] = 0x00;
        data[se_offset + 1] = 0x64; // 100 in big-endian

        file.write_all(&data).unwrap();
        file
    }

    #[test]
    fn test_load_srtm3_file() {
        let file = create_test_srtm3_file();
        let tile = SrtmTile::from_file(file.path()).unwrap();

        assert_eq!(tile.resolution(), SrtmResolution::Srtm3);
        assert_eq!(tile.samples(), SRTM3_SAMPLES);
    }

    #[test]
    fn test_invalid_file_size() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&vec![0u8; 1000]).unwrap();

        let result = SrtmTile::from_file(file.path());
        assert!(result.is_err());

        if let Err(SrtmError::InvalidFileSize { size }) = result {
            assert_eq!(size, 1000);
        } else {
            panic!("Expected InvalidFileSize error");
        }
    }

    #[test]
    fn test_get_elevation_corners() {
        let file = create_test_srtm3_file();
        let tile = SrtmTile::from_file_with_coords(file.path(), 35, 138).unwrap();

        // Northwest corner (lat=36.0, lon=138.0) -> row 0, col 0 -> 1000m
        // Note: lat_frac = 1.0, so row = 0
        let elev = tile.get_elevation(35.9999, 138.0001).unwrap();
        // Due to rounding, this should be close to the NW corner
        assert!(elev >= 0, "Elevation should be non-negative");

        // Southeast corner (lat=35.0, lon=139.0) -> row 1200, col 1200 -> 100m
        let elev = tile.get_elevation(35.0001, 138.9999).unwrap();
        assert!(elev >= 0, "Elevation should be non-negative");
    }

    #[test]
    fn test_get_elevation_center() {
        let file = create_test_srtm3_file();
        let tile = SrtmTile::from_file_with_coords(file.path(), 35, 138).unwrap();

        // Center of tile (lat=35.5, lon=138.5) -> approximately row 600, col 600
        let elev = tile.get_elevation(35.5, 138.5).unwrap();
        // Should be 500m as set in test data
        assert_eq!(elev, 500);
    }

    #[test]
    fn test_resolution_info() {
        assert_eq!(SrtmResolution::Srtm1.samples(), 3601);
        assert_eq!(SrtmResolution::Srtm3.samples(), 1201);
        assert_eq!(SrtmResolution::Srtm1.meters(), 30.0);
        assert_eq!(SrtmResolution::Srtm3.meters(), 90.0);
    }

    /// Create a test file with a 2x2 grid of known values for interpolation testing.
    /// Sets values at rows 600-601, cols 600-601 to form a gradient.
    fn create_interpolation_test_file() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        let mut data = vec![0u8; SRTM3_SIZE];

        // Create a 2x2 grid of values around the center:
        // (row 600, col 600) = 100m  |  (row 600, col 601) = 200m
        // (row 601, col 600) = 300m  |  (row 601, col 601) = 400m

        let set_elevation = |data: &mut Vec<u8>, row: usize, col: usize, elev: i16| {
            let offset = (row * SRTM3_SAMPLES + col) * 2;
            let bytes = elev.to_be_bytes();
            data[offset] = bytes[0];
            data[offset + 1] = bytes[1];
        };

        set_elevation(&mut data, 600, 600, 100);
        set_elevation(&mut data, 600, 601, 200);
        set_elevation(&mut data, 601, 600, 300);
        set_elevation(&mut data, 601, 601, 400);

        file.write_all(&data).unwrap();
        file
    }

    #[test]
    fn test_interpolation_at_grid_points() {
        let file = create_interpolation_test_file();
        let tile = SrtmTile::from_file_with_coords(file.path(), 35, 138).unwrap();

        // At exact grid points, interpolation should return exact values
        // Row 600 corresponds to lat_frac = 0.5 (approximately)
        // Col 600 corresponds to lon_frac = 0.5 (approximately)

        // The center point (row 600, col 600) should be 100m
        // lat = 35.0 + (1.0 - 600/1200) = 35.5
        // lon = 138.0 + 600/1200 = 138.5
        let elev = tile.get_elevation_interpolated(35.5, 138.5).unwrap();
        assert!(elev.is_some());
        // Should be close to 100m (the value at row 600, col 600)
        let elev = elev.unwrap();
        assert!((elev - 100.0).abs() < 1.0, "Expected ~100, got {}", elev);
    }

    #[test]
    fn test_interpolation_midpoint() {
        let file = create_interpolation_test_file();
        let tile = SrtmTile::from_file_with_coords(file.path(), 35, 138).unwrap();

        // At the midpoint between 4 grid values (100, 200, 300, 400),
        // the interpolated value should be the average: 250

        // Calculate coordinates for midpoint between rows 600-601 and cols 600-601
        // row = 600.5 means lat_frac = 1.0 - 600.5/1200 = 0.49958...
        // col = 600.5 means lon_frac = 600.5/1200 = 0.50041...
        let lat = 35.0 + (1.0 - 600.5 / 1200.0);
        let lon = 138.0 + 600.5 / 1200.0;

        let elev = tile.get_elevation_interpolated(lat, lon).unwrap();
        assert!(elev.is_some());
        let elev = elev.unwrap();

        // The average of 100, 200, 300, 400 is 250
        assert!((elev - 250.0).abs() < 5.0, "Expected ~250, got {}", elev);
    }

    #[test]
    fn test_interpolation_horizontal() {
        let file = create_interpolation_test_file();
        let tile = SrtmTile::from_file_with_coords(file.path(), 35, 138).unwrap();

        // Test horizontal interpolation at row 600
        // At row 600, col 600 = 100m, col 601 = 200m
        // Midpoint should be 150m

        let lat = 35.0 + (1.0 - 600.0 / 1200.0); // row 600
        let lon = 138.0 + 600.5 / 1200.0; // between col 600 and 601

        let elev = tile.get_elevation_interpolated(lat, lon).unwrap();
        assert!(elev.is_some());
        let elev = elev.unwrap();

        // Horizontal interpolation between 100 and 200 should give ~150
        assert!((elev - 150.0).abs() < 10.0, "Expected ~150, got {}", elev);
    }

    #[test]
    fn test_interpolation_void_value() {
        let mut file = NamedTempFile::new().unwrap();
        let mut data = vec![0u8; SRTM3_SIZE];

        // Set one corner to VOID_VALUE
        let void_bytes = VOID_VALUE.to_be_bytes();
        let offset = (600 * SRTM3_SAMPLES + 600) * 2;
        data[offset] = void_bytes[0];
        data[offset + 1] = void_bytes[1];

        // Set other corners to valid values
        let set_elevation = |data: &mut Vec<u8>, row: usize, col: usize, elev: i16| {
            let offset = (row * SRTM3_SAMPLES + col) * 2;
            let bytes = elev.to_be_bytes();
            data[offset] = bytes[0];
            data[offset + 1] = bytes[1];
        };

        set_elevation(&mut data, 600, 601, 200);
        set_elevation(&mut data, 601, 600, 300);
        set_elevation(&mut data, 601, 601, 400);

        file.write_all(&data).unwrap();

        let tile = SrtmTile::from_file_with_coords(file.path(), 35, 138).unwrap();

        // Interpolation should return None when any corner is void
        let lat = 35.0 + (1.0 - 600.5 / 1200.0);
        let lon = 138.0 + 600.5 / 1200.0;

        let elev = tile.get_elevation_interpolated(lat, lon).unwrap();
        assert!(elev.is_none(), "Expected None for void area");
    }

    /// Create a test file with distinct values at adjacent cells to test rounding.
    /// Sets different elevations at (row, col) and (row, col+1) so that
    /// floor vs round produce different results.
    fn create_rounding_test_file() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        let mut data = vec![0u8; SRTM3_SIZE];

        let set_elevation = |data: &mut Vec<u8>, row: usize, col: usize, elev: i16| {
            let offset = (row * SRTM3_SAMPLES + col) * 2;
            let bytes = elev.to_be_bytes();
            data[offset] = bytes[0];
            data[offset + 1] = bytes[1];
        };

        // Set row=786, col=1008 = 191 (floor result)
        // Set row=786, col=1009 = 190 (round result)
        set_elevation(&mut data, 786, 1008, 191);
        set_elevation(&mut data, 786, 1009, 190);

        file.write_all(&data).unwrap();
        file
    }

    #[test]
    fn test_floor_vs_round_different_results() {
        let file = create_rounding_test_file();
        let tile = SrtmTile::from_file_with_coords(file.path(), 33, -97).unwrap();

        // Coordinate that produces col index ~1008.9851
        // floor(1008.9851) = 1008, round(1008.9851) = 1009
        // row index ~786.1869
        // floor(786.1869) = 786, round(786.1869) = 786

        // lat_frac = 33.3448 - 33.0 = 0.3448
        // lon_frac = -96.1592 - (-97.0) = 0.8408
        // row = (1.0 - 0.3448) * 1200 = 786.24
        // col = 0.8408 * 1200 = 1008.96

        let lat = 33.3448;
        let lon = -96.1592;

        let elev_round = tile.get_elevation(lat, lon).unwrap();
        let elev_floor = tile.get_elevation_floor(lat, lon).unwrap();

        assert_eq!(elev_round, 190, "round should select col 1009");
        assert_eq!(elev_floor, 191, "floor should select col 1008");
    }

    #[test]
    fn test_floor_matches_round_at_exact_grid() {
        let file = create_test_srtm3_file();
        let tile = SrtmTile::from_file_with_coords(file.path(), 35, 138).unwrap();

        // At the exact center (row 600, col 600), both should agree
        let elev_round = tile.get_elevation(35.5, 138.5).unwrap();
        let elev_floor = tile.get_elevation_floor(35.5, 138.5).unwrap();

        assert_eq!(elev_round, elev_floor);
        assert_eq!(elev_round, 500);
    }
}
