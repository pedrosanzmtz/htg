use anyhow::{bail, Context, Result};
use htg::{filename::lat_lon_to_filename, SrtmResolution, SrtmTile, VOID_VALUE};
use std::path::PathBuf;

pub fn run(
    data_dir: Option<PathBuf>,
    tile: String,
    lat: Option<f64>,
    lon: Option<f64>,
) -> Result<()> {
    // Determine tile filename
    let (filename, tile_path) = if let (Some(lat), Some(lon)) = (lat, lon) {
        let filename = lat_lon_to_filename(lat, lon);
        let path = get_tile_path(data_dir, &filename)?;
        (filename, path)
    } else if tile.ends_with(".hgt") {
        // Full path provided
        let path = PathBuf::from(&tile);
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&tile)
            .to_string();
        (filename, path)
    } else {
        // Just tile name (e.g., "N35E138")
        let filename = format!("{}.hgt", tile);
        let path = get_tile_path(data_dir, &filename)?;
        (filename, path)
    };

    // Check if file exists
    if !tile_path.exists() {
        bail!("Tile not found: {}", tile_path.display());
    }

    // Parse coordinates from filename first (needed for elevation sampling)
    let (base_lat, base_lon) = htg::filename::filename_to_lat_lon(&filename).unwrap_or((0, 0));

    // Load tile with coordinates
    let tile = SrtmTile::from_file_with_coords(&tile_path, base_lat, base_lon)
        .context("Failed to load tile")?;

    // Get file metadata
    let metadata = std::fs::metadata(&tile_path)?;
    let file_size = metadata.len();

    // Calculate min/max elevation by sampling through the grid
    let samples = tile.samples();
    let (mut min_elev, mut max_elev) = (i16::MAX, i16::MIN);
    let mut void_count = 0u64;

    // Iterate through all samples using lat/lon coordinates
    // Row 0 = north edge, Row samples-1 = south edge
    // Col 0 = west edge, Col samples-1 = east edge
    for row in 0..samples {
        for col in 0..samples {
            // Calculate lat/lon for this sample
            let lat = (base_lat as f64) + 1.0 - (row as f64 / (samples - 1) as f64);
            let lon = (base_lon as f64) + (col as f64 / (samples - 1) as f64);

            if let Ok(elev) = tile.get_elevation(lat, lon) {
                if elev == VOID_VALUE {
                    void_count += 1;
                } else {
                    min_elev = min_elev.min(elev);
                    max_elev = max_elev.max(elev);
                }
            }
        }
    }

    // Format resolution string
    let resolution_str = match tile.resolution() {
        SrtmResolution::Srtm1 => "SRTM1 (~30m)",
        SrtmResolution::Srtm3 => "SRTM3 (~90m)",
    };

    // Display information
    println!("Tile: {}", filename);
    println!("Path: {}", tile_path.display());
    println!();
    println!(
        "Resolution: {} ({}x{} samples)",
        resolution_str, samples, samples
    );
    println!(
        "Coverage: {}{}-{}{}, {}{}{}{}",
        if base_lat >= 0 { "N" } else { "S" },
        base_lat.abs(),
        if base_lat >= 0 { "N" } else { "S" },
        (base_lat + 1).abs(),
        if base_lon >= 0 { "E" } else { "W" },
        base_lon.abs(),
        if base_lon >= 0 { "E" } else { "W" },
        (base_lon + 1).abs()
    );
    println!("File size: {}", format_size(file_size));
    println!();

    if min_elev <= max_elev {
        println!("Min elevation: {}m", min_elev);
        println!("Max elevation: {}m", max_elev);
    }

    let total_samples = (samples * samples) as u64;
    if void_count > 0 {
        let void_pct = (void_count as f64 / total_samples as f64) * 100.0;
        println!("Void samples: {} ({:.1}%)", void_count, void_pct);
    }

    Ok(())
}

fn get_tile_path(data_dir: Option<PathBuf>, filename: &str) -> Result<PathBuf> {
    match data_dir {
        Some(dir) => Ok(dir.join(filename)),
        None => {
            let dir = std::env::var("HTG_DATA_DIR").context(
                "HTG_DATA_DIR environment variable not set. Use --data-dir or set HTG_DATA_DIR",
            )?;
            Ok(PathBuf::from(dir).join(filename))
        }
    }
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
