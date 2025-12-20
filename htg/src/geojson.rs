//! GeoJSON elevation enrichment.
//!
//! This module provides functions to add elevation data to GeoJSON geometries.
//! Enable the `geojson` feature to use this module.
//!
//! # Example
//!
//! ```ignore
//! use htg::SrtmService;
//! use htg::geojson::add_elevations_to_geometry;
//! use geojson::Geometry;
//!
//! let service = SrtmService::new("/path/to/hgt/files", 100);
//!
//! // Parse a GeoJSON geometry
//! let geometry: Geometry = r#"{"type": "Point", "coordinates": [138.7274, 35.3606]}"#
//!     .parse()
//!     .unwrap();
//!
//! // Add elevation to the geometry
//! let enriched = add_elevations_to_geometry(&service, geometry)?;
//! // Result: {"type": "Point", "coordinates": [138.7274, 35.3606, 3776.0]}
//! ```

use geojson::{Geometry, Value as GeoJsonValue};

use crate::error::{Result, SrtmError};
use crate::SrtmService;

/// Add elevations to all coordinates in a GeoJSON geometry.
///
/// This function traverses the geometry and adds elevation (Z coordinate) to
/// every coordinate pair. The input coordinates should be in GeoJSON order:
/// `[longitude, latitude]` or `[longitude, latitude, altitude]`.
///
/// Supported geometry types:
/// - Point
/// - MultiPoint
/// - LineString
/// - MultiLineString
/// - Polygon
/// - MultiPolygon
/// - GeometryCollection
///
/// # Arguments
///
/// * `service` - The SRTM service to query elevations from
/// * `geometry` - The GeoJSON geometry to enrich with elevations
///
/// # Returns
///
/// A new geometry with elevation added as the Z coordinate to all points.
///
/// # Errors
///
/// Returns an error if:
/// - Any coordinate is outside SRTM coverage (±60° latitude)
/// - A required tile file is not available
/// - A coordinate has fewer than 2 elements
///
/// # Example
///
/// ```ignore
/// use htg::geojson::add_elevations_to_geometry;
/// use geojson::Geometry;
///
/// let line: Geometry = r#"{
///     "type": "LineString",
///     "coordinates": [[138.5, 35.5], [138.6, 35.6]]
/// }"#.parse().unwrap();
///
/// let enriched = add_elevations_to_geometry(&service, line)?;
/// // Each coordinate now has elevation: [[138.5, 35.5, 500.0], [138.6, 35.6, 750.0]]
/// ```
pub fn add_elevations_to_geometry(service: &SrtmService, geometry: Geometry) -> Result<Geometry> {
    let new_value = match geometry.value {
        GeoJsonValue::Point(coord) => {
            let elevated = add_elevation_to_coord(service, &coord)?;
            GeoJsonValue::Point(elevated)
        }
        GeoJsonValue::MultiPoint(coords) => {
            let elevated = add_elevation_to_coords(service, &coords)?;
            GeoJsonValue::MultiPoint(elevated)
        }
        GeoJsonValue::LineString(coords) => {
            let elevated = add_elevation_to_coords(service, &coords)?;
            GeoJsonValue::LineString(elevated)
        }
        GeoJsonValue::MultiLineString(lines) => {
            let elevated: Result<Vec<_>> = lines
                .iter()
                .map(|line| add_elevation_to_coords(service, line))
                .collect();
            GeoJsonValue::MultiLineString(elevated?)
        }
        GeoJsonValue::Polygon(rings) => {
            let elevated: Result<Vec<_>> = rings
                .iter()
                .map(|ring| add_elevation_to_coords(service, ring))
                .collect();
            GeoJsonValue::Polygon(elevated?)
        }
        GeoJsonValue::MultiPolygon(polygons) => {
            let elevated: Result<Vec<_>> = polygons
                .iter()
                .map(|polygon| {
                    polygon
                        .iter()
                        .map(|ring| add_elevation_to_coords(service, ring))
                        .collect::<Result<Vec<_>>>()
                })
                .collect();
            GeoJsonValue::MultiPolygon(elevated?)
        }
        GeoJsonValue::GeometryCollection(geometries) => {
            let elevated: Result<Vec<_>> = geometries
                .into_iter()
                .map(|g| add_elevations_to_geometry(service, g))
                .collect();
            GeoJsonValue::GeometryCollection(elevated?)
        }
    };

    Ok(Geometry::new(new_value))
}

/// Add elevation to a single GeoJSON coordinate.
///
/// Takes a coordinate in GeoJSON order `[lon, lat]` or `[lon, lat, alt]` and
/// returns a new coordinate with elevation: `[lon, lat, elevation]`.
///
/// # Arguments
///
/// * `service` - The SRTM service to query elevation from
/// * `coord` - The coordinate as a slice `[lon, lat, ...]`
///
/// # Returns
///
/// A new coordinate vector `[lon, lat, elevation]`.
///
/// # Errors
///
/// Returns an error if:
/// - The coordinate has fewer than 2 elements
/// - The coordinate is outside SRTM coverage
/// - The tile file is not available
///
/// # Example
///
/// ```ignore
/// let coord = vec![138.7274, 35.3606];
/// let elevated = add_elevation_to_coord(&service, &coord)?;
/// assert_eq!(elevated.len(), 3);
/// println!("Elevation: {}m", elevated[2]);
/// ```
pub fn add_elevation_to_coord(service: &SrtmService, coord: &[f64]) -> Result<Vec<f64>> {
    if coord.len() < 2 {
        return Err(SrtmError::InvalidCoordinate {
            message: "Coordinate must have at least 2 elements (lon, lat)".to_string(),
        });
    }

    let lon = coord[0];
    let lat = coord[1];

    let elevation = service.get_elevation(lat, lon)?;

    Ok(vec![lon, lat, elevation as f64])
}

/// Add elevations to a list of GeoJSON coordinates.
///
/// Processes each coordinate in the list, adding elevation to each one.
///
/// # Arguments
///
/// * `service` - The SRTM service to query elevations from
/// * `coords` - A slice of coordinates, each in GeoJSON order `[lon, lat, ...]`
///
/// # Returns
///
/// A vector of coordinates with elevation added.
///
/// # Errors
///
/// Returns an error if any coordinate fails elevation lookup.
pub fn add_elevation_to_coords(
    service: &SrtmService,
    coords: &[Vec<f64>],
) -> Result<Vec<Vec<f64>>> {
    coords
        .iter()
        .map(|coord| add_elevation_to_coord(service, coord))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::TempDir;

    const SRTM3_SIZE: usize = 1201 * 1201 * 2;
    const SRTM3_SAMPLES: usize = 1201;

    fn create_test_tile(dir: &Path, filename: &str, center_elevation: i16) {
        let mut data = vec![0u8; SRTM3_SIZE];

        // Set center elevation (row 600, col 600)
        let center_offset = (600 * SRTM3_SAMPLES + 600) * 2;
        let bytes = center_elevation.to_be_bytes();
        data[center_offset] = bytes[0];
        data[center_offset + 1] = bytes[1];

        let path = dir.join(filename);
        let mut file = fs::File::create(path).unwrap();
        file.write_all(&data).unwrap();
    }

    #[test]
    fn test_add_elevation_to_coord() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        // GeoJSON order: [lon, lat]
        let coord = vec![138.5, 35.5];
        let result = add_elevation_to_coord(&service, &coord).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], 138.5); // lon preserved
        assert_eq!(result[1], 35.5); // lat preserved
        assert_eq!(result[2], 500.0); // elevation added
    }

    #[test]
    fn test_add_elevation_to_coord_invalid() {
        let temp_dir = TempDir::new().unwrap();
        let service = SrtmService::new(temp_dir.path(), 10);

        // Too few elements
        let coord = vec![138.5];
        let result = add_elevation_to_coord(&service, &coord);
        assert!(result.is_err());
    }

    #[test]
    fn test_add_elevation_to_coords() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        let coords = vec![vec![138.5, 35.5], vec![138.6, 35.6]];
        let result = add_elevation_to_coords(&service, &coords).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 3);
        assert_eq!(result[1].len(), 3);
    }

    #[test]
    fn test_add_elevations_to_point() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        let geometry = Geometry::new(GeoJsonValue::Point(vec![138.5, 35.5]));
        let result = add_elevations_to_geometry(&service, geometry).unwrap();

        if let GeoJsonValue::Point(coord) = result.value {
            assert_eq!(coord.len(), 3);
            assert_eq!(coord[2], 500.0);
        } else {
            panic!("Expected Point geometry");
        }
    }

    #[test]
    fn test_add_elevations_to_linestring() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        let geometry = Geometry::new(GeoJsonValue::LineString(vec![
            vec![138.5, 35.5],
            vec![138.6, 35.6],
        ]));
        let result = add_elevations_to_geometry(&service, geometry).unwrap();

        if let GeoJsonValue::LineString(coords) = result.value {
            assert_eq!(coords.len(), 2);
            assert_eq!(coords[0].len(), 3);
            assert_eq!(coords[1].len(), 3);
        } else {
            panic!("Expected LineString geometry");
        }
    }

    #[test]
    fn test_add_elevations_to_polygon() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        // Simple polygon (triangle)
        let geometry = Geometry::new(GeoJsonValue::Polygon(vec![vec![
            vec![138.5, 35.5],
            vec![138.6, 35.5],
            vec![138.55, 35.6],
            vec![138.5, 35.5], // closed ring
        ]]));
        let result = add_elevations_to_geometry(&service, geometry).unwrap();

        if let GeoJsonValue::Polygon(rings) = result.value {
            assert_eq!(rings.len(), 1);
            assert_eq!(rings[0].len(), 4);
            for coord in &rings[0] {
                assert_eq!(coord.len(), 3);
            }
        } else {
            panic!("Expected Polygon geometry");
        }
    }

    #[test]
    fn test_add_elevations_to_geometry_collection() {
        let temp_dir = TempDir::new().unwrap();
        create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

        let service = SrtmService::new(temp_dir.path(), 10);

        let geometry = Geometry::new(GeoJsonValue::GeometryCollection(vec![
            Geometry::new(GeoJsonValue::Point(vec![138.5, 35.5])),
            Geometry::new(GeoJsonValue::LineString(vec![
                vec![138.5, 35.5],
                vec![138.6, 35.6],
            ])),
        ]));
        let result = add_elevations_to_geometry(&service, geometry).unwrap();

        if let GeoJsonValue::GeometryCollection(geometries) = result.value {
            assert_eq!(geometries.len(), 2);
        } else {
            panic!("Expected GeometryCollection");
        }
    }
}
