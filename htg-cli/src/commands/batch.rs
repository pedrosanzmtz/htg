use anyhow::{bail, Context, Result};
use htg::{download::DownloadConfig, SrtmServiceBuilder};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn run(
    data_dir: Option<PathBuf>,
    cache_size: u64,
    auto_download: bool,
    input: PathBuf,
    output: Option<PathBuf>,
    lat_col: String,
    lon_col: String,
    interpolate: bool,
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

    // Detect file format
    let extension = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "csv" => process_csv(&service, &input, output, &lat_col, &lon_col, interpolate),
        "geojson" | "json" => process_geojson(&service, &input, output, interpolate),
        _ => bail!(
            "Unsupported file format: {}. Use .csv or .geojson",
            extension
        ),
    }
}

fn process_csv(
    service: &htg::SrtmService,
    input: &PathBuf,
    output: Option<PathBuf>,
    lat_col: &str,
    lon_col: &str,
    interpolate: bool,
) -> Result<()> {
    let file = File::open(input).context("Failed to open input file")?;
    let mut reader = csv::Reader::from_reader(BufReader::new(file));

    // Find column indices
    let headers = reader.headers()?.clone();
    let lat_idx = headers
        .iter()
        .position(|h| h == lat_col)
        .with_context(|| format!("Column '{}' not found in CSV", lat_col))?;
    let lon_idx = headers
        .iter()
        .position(|h| h == lon_col)
        .with_context(|| format!("Column '{}' not found in CSV", lon_col))?;

    // Collect records for progress bar
    let records: Vec<_> = reader.records().collect::<Result<_, _>>()?;
    let total = records.len() as u64;

    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )?
            .progress_chars("#>-"),
    );

    // Prepare output
    let output_path = output.unwrap_or_else(|| {
        let stem = input.file_stem().unwrap().to_string_lossy();
        input.with_file_name(format!("{}_elevation.csv", stem))
    });
    let output_file = File::create(&output_path).context("Failed to create output file")?;
    let mut writer = csv::Writer::from_writer(BufWriter::new(output_file));

    // Write header
    let mut new_headers: Vec<&str> = headers.iter().collect();
    new_headers.push("elevation");
    writer.write_record(&new_headers)?;

    // Process records
    for record in records {
        let lat: f64 = record
            .get(lat_idx)
            .context("Missing latitude")?
            .parse()
            .context("Invalid latitude")?;
        let lon: f64 = record
            .get(lon_idx)
            .context("Missing longitude")?
            .parse()
            .context("Invalid longitude")?;

        let elevation = if interpolate {
            service
                .get_elevation_interpolated(lat, lon)
                .ok()
                .flatten()
                .map(|e| format!("{:.2}", e))
                .unwrap_or_else(|| "void".to_string())
        } else {
            service
                .get_elevation(lat, lon)
                .ok()
                .flatten()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "void".to_string())
        };

        let mut new_record: Vec<&str> = record.iter().collect();
        new_record.push(&elevation);
        writer.write_record(&new_record)?;

        pb.inc(1);
    }

    pb.finish_with_message("done");
    writer.flush()?;

    println!("Output written to: {}", output_path.display());
    Ok(())
}

fn process_geojson(
    service: &htg::SrtmService,
    input: &PathBuf,
    output: Option<PathBuf>,
    interpolate: bool,
) -> Result<()> {
    let file = File::open(input).context("Failed to open input file")?;
    let reader = BufReader::new(file);

    let geojson: geojson::GeoJson =
        serde_json::from_reader(reader).context("Failed to parse GeoJSON")?;

    let result = match geojson {
        geojson::GeoJson::Geometry(geometry) => {
            let enriched = add_elevations_to_geometry(service, geometry, interpolate)?;
            geojson::GeoJson::Geometry(enriched)
        }
        geojson::GeoJson::Feature(mut feature) => {
            if let Some(geometry) = feature.geometry.take() {
                feature.geometry =
                    Some(add_elevations_to_geometry(service, geometry, interpolate)?);
            }
            geojson::GeoJson::Feature(feature)
        }
        geojson::GeoJson::FeatureCollection(mut fc) => {
            let pb = ProgressBar::new(fc.features.len() as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")?
                    .progress_chars("#>-"),
            );

            for feature in &mut fc.features {
                if let Some(geometry) = feature.geometry.take() {
                    feature.geometry =
                        Some(add_elevations_to_geometry(service, geometry, interpolate)?);
                }
                pb.inc(1);
            }
            pb.finish_with_message("done");
            geojson::GeoJson::FeatureCollection(fc)
        }
    };

    // Write output
    let output_path = output.unwrap_or_else(|| {
        let stem = input.file_stem().unwrap().to_string_lossy();
        input.with_file_name(format!("{}_elevation.geojson", stem))
    });
    let output_file = File::create(&output_path).context("Failed to create output file")?;
    let mut writer = BufWriter::new(output_file);
    serde_json::to_writer_pretty(&mut writer, &result)?;
    writer.flush()?;

    println!("Output written to: {}", output_path.display());
    Ok(())
}

fn add_elevations_to_geometry(
    service: &htg::SrtmService,
    geometry: geojson::Geometry,
    interpolate: bool,
) -> Result<geojson::Geometry> {
    use geojson::Value;

    fn add_elevation_to_position(
        service: &htg::SrtmService,
        pos: &mut Vec<f64>,
        interpolate: bool,
    ) {
        if pos.len() >= 2 {
            let lon = pos[0];
            let lat = pos[1];
            let elevation = if interpolate {
                service
                    .get_elevation_interpolated(lat, lon)
                    .ok()
                    .flatten()
                    .unwrap_or(0.0)
            } else {
                service.get_elevation(lat, lon).ok().flatten().unwrap_or(0) as f64
            };
            if pos.len() == 2 {
                pos.push(elevation);
            } else {
                pos[2] = elevation;
            }
        }
    }

    fn process_positions(
        service: &htg::SrtmService,
        positions: &mut Vec<Vec<f64>>,
        interpolate: bool,
    ) {
        for pos in positions {
            add_elevation_to_position(service, pos, interpolate);
        }
    }

    fn process_line_string(
        service: &htg::SrtmService,
        coords: &mut Vec<Vec<f64>>,
        interpolate: bool,
    ) {
        process_positions(service, coords, interpolate);
    }

    fn process_polygon(
        service: &htg::SrtmService,
        rings: &mut Vec<Vec<Vec<f64>>>,
        interpolate: bool,
    ) {
        for ring in rings {
            process_positions(service, ring, interpolate);
        }
    }

    let value = match geometry.value {
        Value::Point(mut coords) => {
            add_elevation_to_position(service, &mut coords, interpolate);
            Value::Point(coords)
        }
        Value::MultiPoint(mut coords) => {
            process_positions(service, &mut coords, interpolate);
            Value::MultiPoint(coords)
        }
        Value::LineString(mut coords) => {
            process_line_string(service, &mut coords, interpolate);
            Value::LineString(coords)
        }
        Value::MultiLineString(mut lines) => {
            for line in &mut lines {
                process_line_string(service, line, interpolate);
            }
            Value::MultiLineString(lines)
        }
        Value::Polygon(mut rings) => {
            process_polygon(service, &mut rings, interpolate);
            Value::Polygon(rings)
        }
        Value::MultiPolygon(mut polys) => {
            for poly in &mut polys {
                process_polygon(service, poly, interpolate);
            }
            Value::MultiPolygon(polys)
        }
        Value::GeometryCollection(geometries) => {
            let mut new_geometries = Vec::new();
            for geom in geometries {
                new_geometries.push(add_elevations_to_geometry(service, geom, interpolate)?);
            }
            Value::GeometryCollection(new_geometries)
        }
    };

    Ok(geojson::Geometry::new(value))
}
