//! Example demonstrating bilinear interpolation for smoother elevation queries.
//!
//! Run with: cargo run --example interpolation -- /path/to/hgt/files

use htg::{SrtmService, SrtmError};
use std::env;

fn main() -> Result<(), SrtmError> {
    let data_dir = env::args()
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("Usage: cargo run --example interpolation -- /path/to/hgt/files");
            std::process::exit(1);
        });

    let service = SrtmService::new(&data_dir, 10);

    // Compare nearest-neighbor vs interpolated elevation
    let lat = 35.3606;
    let lon = 138.7274;

    println!("Comparing elevation methods at ({}, {}):", lat, lon);
    println!("{:-<50}", "");

    // Nearest-neighbor lookup
    match service.get_elevation(lat, lon) {
        Ok(elevation) => {
            println!("Nearest-neighbor: {}m", elevation);
        }
        Err(e) => {
            println!("Error: {}", e);
            return Ok(());
        }
    }

    // Bilinear interpolation
    match service.get_elevation_interpolated(lat, lon) {
        Ok(Some(elevation)) => {
            println!("Interpolated:     {:.2}m", elevation);
        }
        Ok(None) => {
            println!("Interpolated:     void (no data)");
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }

    Ok(())
}
