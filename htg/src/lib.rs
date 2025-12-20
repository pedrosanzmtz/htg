//! # HTG - SRTM Elevation Library
//!
//! High-performance, memory-efficient library for querying elevation data from
//! SRTM (Shuttle Radar Topography Mission) `.hgt` files.
//!
//! ## Features
//!
//! - **Fast**: Memory-mapped I/O for instant data access
//! - **Memory Efficient**: Only loads tiles on demand
//! - **Automatic Detection**: Determines tile resolution (SRTM1/SRTM3) from file size
//! - **Offline**: Works with local `.hgt` files, no internet required
//!
//! ## Quick Start
//!
//! ```ignore
//! use htg::{SrtmTile, filename};
//!
//! // Determine which file to load
//! let filename = filename::lat_lon_to_filename(35.5, 138.7);
//! assert_eq!(filename, "N35E138.hgt");
//!
//! // Load the tile and query elevation
//! let tile = SrtmTile::from_file(&format!("/data/{}", filename))?;
//! let elevation = tile.get_elevation(35.5, 138.7)?;
//! println!("Elevation: {}m", elevation);
//! ```
//!
//! ## SRTM Data Format
//!
//! SRTM files contain elevation data in a simple binary format:
//!
//! - **SRTM1**: 3601×3601 samples, 1 arc-second (~30m) resolution
//! - **SRTM3**: 1201×1201 samples, 3 arc-second (~90m) resolution
//!
//! Each sample is a 16-bit big-endian signed integer representing elevation in meters.
//! The special value -32768 indicates void (no data).
//!
//! ## Data Sources
//!
//! Download SRTM data from:
//! - <https://dwtkns.com/srtm30m/>
//! - <https://earthexplorer.usgs.gov/>

pub mod error;
pub mod filename;
pub mod tile;

// Re-export main types at crate root for convenience
pub use error::{Result, SrtmError};
pub use tile::{SrtmResolution, SrtmTile, VOID_VALUE};
