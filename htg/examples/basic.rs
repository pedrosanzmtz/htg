//! Basic example demonstrating htg library usage.
//!
//! Run with: cargo run --example basic -- /path/to/hgt/files

use htg::{SrtmService, SrtmError};
use std::env;

fn main() -> Result<(), SrtmError> {
    // Get data directory from command line
    let data_dir = env::args()
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("Usage: cargo run --example basic -- /path/to/hgt/files");
            std::process::exit(1);
        });

    // Create service with up to 10 cached tiles
    let service = SrtmService::new(&data_dir, 10);

    // Query some famous peaks
    let locations = [
        ("Mount Fuji, Japan", 35.3606, 138.7274),
        ("Mount Everest, Nepal", 27.9881, 86.9250),
        ("Denali, Alaska", 63.0695, -151.0074),
    ];

    println!("Elevation queries (nearest-neighbor):");
    println!("{:-<50}", "");

    for (name, lat, lon) in &locations {
        match service.get_elevation(*lat, *lon) {
            Ok(elevation) => {
                println!("{}: {}m", name, elevation);
            }
            Err(SrtmError::FileNotFound { .. }) => {
                println!("{}: tile not available locally", name);
            }
            Err(e) => {
                println!("{}: error - {}", name, e);
            }
        }
    }

    // Show cache statistics
    let stats = service.cache_stats();
    println!("\nCache statistics:");
    println!("  Cached tiles: {}", stats.entry_count);
    println!("  Hits: {}", stats.hit_count);
    println!("  Misses: {}", stats.miss_count);
    println!("  Hit rate: {:.1}%", stats.hit_rate() * 100.0);

    Ok(())
}
