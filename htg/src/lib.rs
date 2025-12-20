//! # HTG - SRTM Elevation Library
//!
//! High-performance, memory-efficient library for querying elevation data from
//! SRTM (Shuttle Radar Topography Mission) `.hgt` files.
//!
//! ## Features
//!
//! - **Fast**: Memory-mapped I/O for instant data access
//! - **Memory Efficient**: LRU cache limits memory usage
//! - **Automatic Detection**: Determines tile resolution (SRTM1/SRTM3) from file size
//! - **Offline**: Works with local `.hgt` files, no internet required
//!
//! ## Quick Start
//!
//! The easiest way to use htg is through [`SrtmService`], which handles tile
//! loading and caching automatically:
//!
//! ```ignore
//! use htg::SrtmService;
//!
//! // Create service with up to 100 cached tiles
//! let service = SrtmService::new("/path/to/hgt/files", 100);
//!
//! // Query elevation - tile loading is automatic
//! let elevation = service.get_elevation(35.6762, 139.6503)?; // Tokyo
//! println!("Elevation: {}m", elevation);
//!
//! // Check cache performance
//! let stats = service.cache_stats();
//! println!("Cache hit rate: {:.1}%", stats.hit_rate() * 100.0);
//! ```
//!
//! ## Low-Level API
//!
//! For more control, you can work with tiles directly:
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
pub mod service;
pub mod tile;

// Re-export main types at crate root for convenience
pub use error::{Result, SrtmError};
pub use service::{CacheStats, SrtmService};
pub use tile::{SrtmResolution, SrtmTile, VOID_VALUE};
