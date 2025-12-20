//! HTTP request handlers for the elevation service.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

/// Query parameters for elevation endpoint.
#[derive(Debug, Deserialize)]
pub struct ElevationQuery {
    /// Latitude in decimal degrees (-60 to 60).
    pub lat: f64,
    /// Longitude in decimal degrees (-180 to 180).
    pub lon: f64,
    /// Whether to use bilinear interpolation for sub-pixel accuracy.
    /// When true, returns a floating-point elevation value.
    /// Default is false (nearest-neighbor lookup).
    #[serde(default)]
    pub interpolate: bool,
}

/// Successful elevation response.
#[derive(Debug, Serialize)]
pub struct ElevationResponse {
    /// Elevation in meters (integer, nearest-neighbor lookup).
    pub elevation: i16,
    /// Latitude queried.
    pub lat: f64,
    /// Longitude queried.
    pub lon: f64,
}

/// Successful interpolated elevation response.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// Error message.
    pub error: String,
}

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Service status.
    pub status: String,
    /// Service version.
    pub version: String,
}

/// Cache statistics response.
#[derive(Debug, Serialize)]
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
/// # Query Parameters
///
/// - `lat`: Latitude in decimal degrees (-60 to 60)
/// - `lon`: Longitude in decimal degrees (-180 to 180)
/// - `interpolate`: Optional boolean to enable bilinear interpolation (default: false)
///
/// # Returns
///
/// - `200 OK` with elevation data on success
/// - `400 Bad Request` if coordinates are invalid
/// - `404 Not Found` if tile data is unavailable
/// - `500 Internal Server Error` on unexpected errors
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

/// Health check endpoint.
///
/// Returns service status and version.
pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Get cache statistics.
///
/// Returns information about the tile cache.
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
