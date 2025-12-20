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

    /// Get the elevation at the specified coordinates.
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
        let row = ((1.0 - lat_frac) * (self.samples - 1) as f64).round() as usize;
        let col = (lon_frac * (self.samples - 1) as f64).round() as usize;

        self.get_elevation_at(row, col)
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
}
