//! HTTP request handlers for the elevation service.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use geojson::{Geometry, Value as GeoJsonValue};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::AppState;

/// Query parameters for elevation endpoint.
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct ElevationQuery {
    /// Latitude in decimal degrees (-60 to 60).
    #[param(example = 35.3606)]
    pub lat: f64,
    /// Longitude in decimal degrees (-180 to 180).
    #[param(example = 138.7274)]
    pub lon: f64,
    /// Whether to use bilinear interpolation for sub-pixel accuracy.
    /// When true, returns a floating-point elevation value.
    /// Default is false (nearest-neighbor lookup).
    #[serde(default)]
    #[param(example = false)]
    pub interpolate: bool,
}

/// Successful elevation response.
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({"elevation": 3776, "lat": 35.3606, "lon": 138.7274}))]
pub struct ElevationResponse {
    /// Elevation in meters (integer, nearest-neighbor lookup).
    pub elevation: i16,
    /// Latitude queried.
    pub lat: f64,
    /// Longitude queried.
    pub lon: f64,
}

/// Successful interpolated elevation response.
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({"elevation": 3776.42, "lat": 35.3606, "lon": 138.7274, "interpolated": true}))]
pub struct InterpolatedElevationResponse {
    /// Elevation in meters (floating-point, bilinear interpolation).
    pub elevation: f64,
    /// Latitude queried.
    pub lat: f64,
    /// Longitude queried.
    pub lon: f64,
    /// Whether interpolation was used.
    pub interpolated: bool,
}

/// Error response.
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({"error": "Coordinates out of bounds: lat=91, lon=0"}))]
pub struct ErrorResponse {
    /// Error message.
    pub error: String,
}

/// Health check response.
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({"status": "healthy", "version": "0.1.0"}))]
pub struct HealthResponse {
    /// Service status.
    pub status: String,
    /// Service version.
    pub version: String,
}

/// Cache statistics response.
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({"cached_tiles": 45, "cache_hits": 1234, "cache_misses": 56, "hit_rate": 0.956}))]
pub struct StatsResponse {
    /// Number of tiles in cache.
    pub cached_tiles: u64,
    /// Cache hit count.
    pub cache_hits: u64,
    /// Cache miss count.
    pub cache_misses: u64,
    /// Cache hit rate (0.0 to 1.0).
    pub hit_rate: f64,
}

/// Get elevation for given coordinates.
///
/// Returns the elevation in meters for the specified latitude and longitude.
/// Optionally uses bilinear interpolation for sub-pixel accuracy.
#[utoipa::path(
    get,
    path = "/elevation",
    params(ElevationQuery),
    responses(
        (status = 200, description = "Elevation found", body = ElevationResponse),
        (status = 200, description = "Interpolated elevation found", body = InterpolatedElevationResponse),
        (status = 400, description = "Invalid coordinates", body = ErrorResponse),
        (status = 404, description = "Tile not available", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    tag = "elevation"
)]
#[axum::debug_handler]
pub async fn get_elevation(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ElevationQuery>,
) -> impl IntoResponse {
    tracing::debug!(
        lat = query.lat,
        lon = query.lon,
        interpolate = query.interpolate,
        "Elevation query"
    );

    if query.interpolate {
        // Use bilinear interpolation
        match state
            .srtm_service
            .get_elevation_interpolated(query.lat, query.lon)
        {
            Ok(Some(elevation)) => {
                tracing::info!(
                    lat = query.lat,
                    lon = query.lon,
                    elevation = elevation,
                    interpolated = true,
                    "Elevation found"
                );
                (
                    StatusCode::OK,
                    Json(InterpolatedElevationResponse {
                        elevation,
                        lat: query.lat,
                        lon: query.lon,
                        interpolated: true,
                    }),
                )
                    .into_response()
            }
            Ok(None) => {
                // Void value in interpolation area - fall back to nearest neighbor
                match state.srtm_service.get_elevation(query.lat, query.lon) {
                    Ok(elevation) => {
                        tracing::info!(
                            lat = query.lat,
                            lon = query.lon,
                            elevation = elevation,
                            interpolated = false,
                            "Elevation found (void in interpolation area, using nearest)"
                        );
                        (
                            StatusCode::OK,
                            Json(InterpolatedElevationResponse {
                                elevation: elevation as f64,
                                lat: query.lat,
                                lon: query.lon,
                                interpolated: false,
                            }),
                        )
                            .into_response()
                    }
                    Err(e) => error_response(query.lat, query.lon, e),
                }
            }
            Err(e) => error_response(query.lat, query.lon, e),
        }
    } else {
        // Use nearest-neighbor lookup
        match state.srtm_service.get_elevation(query.lat, query.lon) {
            Ok(elevation) => {
                tracing::info!(
                    lat = query.lat,
                    lon = query.lon,
                    elevation = elevation,
                    "Elevation found"
                );
                (
                    StatusCode::OK,
                    Json(ElevationResponse {
                        elevation,
                        lat: query.lat,
                        lon: query.lon,
                    }),
                )
                    .into_response()
            }
            Err(e) => error_response(query.lat, query.lon, e),
        }
    }
}

/// Create an error response for elevation queries.
fn error_response(lat: f64, lon: f64, e: htg::SrtmError) -> axum::response::Response {
    let (status, message) = match &e {
        htg::SrtmError::OutOfBounds { .. } => (StatusCode::BAD_REQUEST, e.to_string()),
        htg::SrtmError::FileNotFound { .. } | htg::SrtmError::TileNotAvailable { .. } => {
            (StatusCode::NOT_FOUND, e.to_string())
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    tracing::warn!(lat = lat, lon = lon, error = %e, "Elevation query failed");

    (status, Json(ErrorResponse { error: message })).into_response()
}

/// Batch elevation query using GeoJSON.
///
/// Accepts GeoJSON geometry and returns the same geometry with elevation
/// added as the Z coordinate to all points.
///
/// Supported geometry types: Point, MultiPoint, LineString, MultiLineString,
/// Polygon, MultiPolygon, GeometryCollection.
#[utoipa::path(
    post,
    path = "/elevation",
    request_body(
        content = serde_json::Value,
        description = "GeoJSON Geometry object",
        example = json!({"type": "LineString", "coordinates": [[138.7274, 35.3606], [138.7300, 35.3650]]})
    ),
    responses(
        (status = 200, description = "Geometry with elevations added", content_type = "application/json"),
        (status = 400, description = "Invalid geometry or coordinates", body = ErrorResponse),
    ),
    tag = "elevation"
)]
#[axum::debug_handler]
pub async fn post_elevation(
    State(state): State<Arc<AppState>>,
    Json(geometry): Json<Geometry>,
) -> impl IntoResponse {
    tracing::debug!(?geometry, "GeoJSON elevation query");

    match add_elevations_to_geometry(&state.srtm_service, geometry) {
        Ok(result) => {
            tracing::info!("GeoJSON elevation query successful");
            (StatusCode::OK, Json(result)).into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "GeoJSON elevation query failed");
            (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response()
        }
    }
}

/// Add elevations to a GeoJSON geometry.
fn add_elevations_to_geometry(
    service: &htg::SrtmService,
    geometry: Geometry,
) -> Result<Geometry, String> {
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
            let elevated: Result<Vec<_>, _> = lines
                .iter()
                .map(|line| add_elevation_to_coords(service, line))
                .collect();
            GeoJsonValue::MultiLineString(elevated?)
        }
        GeoJsonValue::Polygon(rings) => {
            let elevated: Result<Vec<_>, _> = rings
                .iter()
                .map(|ring| add_elevation_to_coords(service, ring))
                .collect();
            GeoJsonValue::Polygon(elevated?)
        }
        GeoJsonValue::MultiPolygon(polygons) => {
            let elevated: Result<Vec<_>, _> = polygons
                .iter()
                .map(|polygon| {
                    polygon
                        .iter()
                        .map(|ring| add_elevation_to_coords(service, ring))
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect();
            GeoJsonValue::MultiPolygon(elevated?)
        }
        GeoJsonValue::GeometryCollection(geometries) => {
            let elevated: Result<Vec<_>, _> = geometries
                .into_iter()
                .map(|g| add_elevations_to_geometry(service, g))
                .collect();
            GeoJsonValue::GeometryCollection(elevated?)
        }
    };

    Ok(Geometry::new(new_value))
}

/// Add elevation to a single coordinate [lon, lat] -> [lon, lat, elevation].
fn add_elevation_to_coord(service: &htg::SrtmService, coord: &[f64]) -> Result<Vec<f64>, String> {
    if coord.len() < 2 {
        return Err("Coordinate must have at least 2 elements (lon, lat)".to_string());
    }

    let lon = coord[0];
    let lat = coord[1];

    let elevation = service.get_elevation(lat, lon).map_err(|e| e.to_string())?;

    Ok(vec![lon, lat, elevation as f64])
}

/// Add elevations to a list of coordinates.
fn add_elevation_to_coords(
    service: &htg::SrtmService,
    coords: &[Vec<f64>],
) -> Result<Vec<Vec<f64>>, String> {
    coords
        .iter()
        .map(|coord| add_elevation_to_coord(service, coord))
        .collect()
}

/// Health check endpoint.
///
/// Returns service status and version.
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
    ),
    tag = "system"
)]
pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Get cache statistics.
///
/// Returns information about the tile cache including hit rate.
#[utoipa::path(
    get,
    path = "/stats",
    responses(
        (status = 200, description = "Cache statistics", body = StatsResponse),
    ),
    tag = "system"
)]
pub async fn get_stats(State(state): State<Arc<AppState>>) -> Json<StatsResponse> {
    let stats = state.srtm_service.cache_stats();

    Json(StatsResponse {
        cached_tiles: stats.entry_count,
        cache_hits: stats.hit_count,
        cache_misses: stats.miss_count,
        hit_rate: stats.hit_rate(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elevation_query_deserialize() {
        let json = r#"{"lat": 35.5, "lon": 138.7}"#;
        let query: ElevationQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.lat, 35.5);
        assert_eq!(query.lon, 138.7);
    }

    #[test]
    fn test_elevation_response_serialize() {
        let response = ElevationResponse {
            elevation: 1234,
            lat: 35.5,
            lon: 138.7,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("1234"));
        assert!(json.contains("35.5"));
    }

    #[test]
    fn test_health_response_serialize() {
        let response = HealthResponse {
            status: "healthy".to_string(),
            version: "0.1.0".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("healthy"));
        assert!(json.contains("0.1.0"));
    }
}
