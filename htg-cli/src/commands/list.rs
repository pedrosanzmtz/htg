use anyhow::{Context, Result};
use htg::filename::filename_to_lat_lon;
use std::fs;
use std::path::PathBuf;

pub fn run(data_dir: Option<PathBuf>) -> Result<()> {
    let dir = match data_dir {
        Some(dir) => dir,
        None => {
            let dir = std::env::var("HTG_DATA_DIR").context(
                "HTG_DATA_DIR environment variable not set. Use --data-dir or set HTG_DATA_DIR",
            )?;
            PathBuf::from(dir)
        }
    };

    if !dir.exists() {
        anyhow::bail!("Data directory does not exist: {}", dir.display());
    }

    // Collect .hgt files
    let mut tiles: Vec<_> = fs::read_dir(&dir)
        .context("Failed to read data directory")?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|e| e == "hgt")
                .unwrap_or(false)
        })
        .collect();

    if tiles.is_empty() {
        println!("No .hgt files found in: {}", dir.display());
        return Ok(());
    }

    // Sort by filename
    tiles.sort_by_key(|e| e.file_name());

    // Detect resolution from file size
    const SRTM1_SIZE: u64 = 3601 * 3601 * 2;
    const SRTM3_SIZE: u64 = 1201 * 1201 * 2;

    let mut srtm1_count = 0;
    let mut srtm3_count = 0;
    let mut unknown_count = 0;
    let mut total_size: u64 = 0;

    println!("{:<12} {:>8} {:>20}", "TILE", "TYPE", "COVERAGE");
    println!("{}", "-".repeat(44));

    for entry in &tiles {
        let filename = entry.file_name();
        let filename_str = filename.to_string_lossy();
        let path = entry.path();

        let metadata = fs::metadata(&path).ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        total_size += size;

        let resolution = match size {
            s if s == SRTM1_SIZE => {
                srtm1_count += 1;
                "SRTM1"
            }
            s if s == SRTM3_SIZE => {
                srtm3_count += 1;
                "SRTM3"
            }
            _ => {
                unknown_count += 1;
                "???"
            }
        };

        // Parse coverage from filename
        let coverage = if let Some((lat, lon)) = filename_to_lat_lon(&filename_str) {
            let lat_prefix = if lat >= 0 { "N" } else { "S" };
            let lon_prefix = if lon >= 0 { "E" } else { "W" };
            format!(
                "{}{:02} to {}{:02}, {}{:03} to {}{:03}",
                lat_prefix,
                lat.abs(),
                lat_prefix,
                (lat + 1).abs(),
                lon_prefix,
                lon.abs(),
                lon_prefix,
                (lon + 1).abs()
            )
        } else {
            "Unknown".to_string()
        };

        println!("{:<12} {:>8} {:>20}", filename_str, resolution, coverage);
    }

    // Summary
    println!();
    println!("Summary:");
    println!("  Total tiles: {}", tiles.len());
    if srtm1_count > 0 {
        println!("  SRTM1 (30m): {}", srtm1_count);
    }
    if srtm3_count > 0 {
        println!("  SRTM3 (90m): {}", srtm3_count);
    }
    if unknown_count > 0 {
        println!("  Unknown: {}", unknown_count);
    }
    println!("  Total size: {}", format_size(total_size));
    println!("  Data directory: {}", dir.display());

    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}
