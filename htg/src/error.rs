//! Error types for the HTG library.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur when working with SRTM data.
#[derive(Error, Debug)]
pub enum SrtmError {
    /// IO error when reading files.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// File size doesn't match SRTM1 or SRTM3 format.
    #[error("Invalid file size: {size} bytes (expected 25934402 for SRTM1 or 2884802 for SRTM3)")]
    InvalidFileSize { size: usize },

    /// Coordinates are outside valid SRTM coverage.
    #[error("Coordinates out of bounds: lat={lat}, lon={lon} (valid: lat ±60°, lon ±180°)")]
    OutOfBounds { lat: f64, lon: f64 },

    /// The required .hgt file was not found.
    #[error("SRTM file not found: {path}")]
    FileNotFound { path: PathBuf },
}

/// Result type alias using [`SrtmError`].
pub type Result<T> = std::result::Result<T, SrtmError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = SrtmError::InvalidFileSize { size: 1000 };
        assert!(err.to_string().contains("1000"));

        let err = SrtmError::OutOfBounds {
            lat: 91.0,
            lon: 0.0,
        };
        assert!(err.to_string().contains("91"));

        let err = SrtmError::FileNotFound {
            path: PathBuf::from("N35E138.hgt"),
        };
        assert!(err.to_string().contains("N35E138.hgt"));
    }
}
