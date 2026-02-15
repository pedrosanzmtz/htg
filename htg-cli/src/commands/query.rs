use anyhow::{Context, Result};
use htg::{download::DownloadConfig, SrtmServiceBuilder};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
struct ElevationResponse {
    lat: f64,
    lon: f64,
    elevation: Option<f64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    interpolated: bool,
}

pub fn run(
    data_dir: Option<PathBuf>,
    cache_size: u64,
    auto_download: bool,
    lat: f64,
    lon: f64,
    interpolate: bool,
    json: bool,
) -> Result<()> {
    // Build the service
    let mut builder = match data_dir {
        Some(dir) => SrtmServiceBuilder::new(dir),
        None => SrtmServiceBuilder::from_env().context(
            "HTG_DATA_DIR environment variable not set. Use --data-dir or set HTG_DATA_DIR",
        )?,
    };

    builder = builder.cache_size(cache_size);

    if auto_download {
        builder = builder.auto_download(DownloadConfig::ardupilot_srtm1());
    }

    let service = builder.build().context("Failed to create SRTM service")?;

    // Query elevation
    let (elevation, is_void) = if interpolate {
        match service
            .get_elevation_interpolated(lat, lon)
            .context("Failed to get elevation")?
        {
            Some(elev) => (Some(elev), false),
            None => (None, true),
        }
    } else {
        match service
            .get_elevation(lat, lon)
            .context("Failed to get elevation")?
        {
            Some(elev) => (Some(elev as f64), false),
            None => (None, true),
        }
    };

    // Output result
    if json {
        let response = ElevationResponse {
            lat,
            lon,
            elevation,
            interpolated: interpolate,
        };
        println!("{}", serde_json::to_string(&response)?);
    } else if is_void {
        println!("void");
    } else if let Some(elev) = elevation {
        if interpolate {
            println!("{:.2}", elev);
        } else {
            println!("{}", elev as i16);
        }
    }

    Ok(())
}
