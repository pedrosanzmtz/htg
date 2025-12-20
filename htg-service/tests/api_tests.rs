//! Integration tests for the HTTP API.

use axum::{routing::get, Router};
use axum_test::TestServer;
use geojson::{Geometry, Value as GeoJsonValue};
use htg::SrtmService;
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use tempfile::TempDir;

/// File size for SRTM3 (1201 × 1201 × 2 bytes)
const SRTM3_SIZE: usize = 1201 * 1201 * 2;
const SRTM3_SAMPLES: usize = 1201;

/// Application state shared across handlers.
pub struct AppState {
    pub srtm_service: SrtmService,
}

/// Create a test SRTM3 file with specified center elevation.
fn create_test_tile(dir: &std::path::Path, filename: &str, center_elevation: i16) {
    let mut data = vec![0u8; SRTM3_SIZE];

    // Set center elevation (row 600, col 600)
    let center_offset = (600 * SRTM3_SAMPLES + 600) * 2;
    let bytes = center_elevation.to_be_bytes();
    data[center_offset] = bytes[0];
    data[center_offset + 1] = bytes[1];

    let path = dir.join(filename);
    let mut file = File::create(path).unwrap();
    file.write_all(&data).unwrap();
}

/// Create a test server with a mock SRTM service.
async fn create_test_server(temp_dir: &TempDir) -> TestServer {
    let srtm_service = SrtmService::new(temp_dir.path(), 10);
    let state = Arc::new(AppState { srtm_service });

    let app = Router::new()
        .route("/elevation", get(get_elevation).post(post_elevation))
        .route("/health", get(health_check))
        .route("/stats", get(get_stats))
        .with_state(state);

    TestServer::new(app).unwrap()
}

// Re-implement handlers for testing (since they're in the binary crate)
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ElevationQuery {
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub interpolate: bool,
}

#[derive(Debug, Serialize)]
pub struct ElevationResponse {
    pub elevation: i16,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Serialize)]
pub struct InterpolatedElevationResponse {
    pub elevation: f64,
    pub lat: f64,
    pub lon: f64,
    pub interpolated: bool,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub cached_tiles: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub hit_rate: f64,
}

async fn get_elevation(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ElevationQuery>,
) -> impl IntoResponse {
    if query.interpolate {
        match state
            .srtm_service
            .get_elevation_interpolated(query.lat, query.lon)
        {
            Ok(Some(elevation)) => (
                StatusCode::OK,
                Json(InterpolatedElevationResponse {
                    elevation,
                    lat: query.lat,
                    lon: query.lon,
                    interpolated: true,
                }),
            )
                .into_response(),
            Ok(None) => {
                // Fall back to nearest neighbor
                match state.srtm_service.get_elevation(query.lat, query.lon) {
                    Ok(elevation) => (
                        StatusCode::OK,
                        Json(InterpolatedElevationResponse {
                            elevation: elevation as f64,
                            lat: query.lat,
                            lon: query.lon,
                            interpolated: false,
                        }),
                    )
                        .into_response(),
                    Err(e) => error_response(e),
                }
            }
            Err(e) => error_response(e),
        }
    } else {
        match state.srtm_service.get_elevation(query.lat, query.lon) {
            Ok(elevation) => (
                StatusCode::OK,
                Json(ElevationResponse {
                    elevation,
                    lat: query.lat,
                    lon: query.lon,
                }),
            )
                .into_response(),
            Err(e) => error_response(e),
        }
    }
}

fn error_response(e: htg::SrtmError) -> axum::response::Response {
    let (status, message) = match &e {
        htg::SrtmError::OutOfBounds { .. } => (StatusCode::BAD_REQUEST, e.to_string()),
        htg::SrtmError::FileNotFound { .. } | htg::SrtmError::TileNotAvailable { .. } => {
            (StatusCode::NOT_FOUND, e.to_string())
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    (status, Json(ErrorResponse { error: message })).into_response()
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn get_stats(State(state): State<Arc<AppState>>) -> Json<StatsResponse> {
    let stats = state.srtm_service.cache_stats();
    Json(StatsResponse {
        cached_tiles: stats.entry_count,
        cache_hits: stats.hit_count,
        cache_misses: stats.miss_count,
        hit_rate: stats.hit_rate(),
    })
}

async fn post_elevation(
    State(state): State<Arc<AppState>>,
    Json(geometry): Json<Geometry>,
) -> impl IntoResponse {
    match add_elevations_to_geometry(&state.srtm_service, geometry) {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
    }
}

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
        _ => return Err("Unsupported geometry type".to_string()),
    };

    Ok(Geometry::new(new_value))
}

fn add_elevation_to_coord(service: &htg::SrtmService, coord: &[f64]) -> Result<Vec<f64>, String> {
    if coord.len() < 2 {
        return Err("Coordinate must have at least 2 elements (lon, lat)".to_string());
    }

    let lon = coord[0];
    let lat = coord[1];

    let elevation = service.get_elevation(lat, lon).map_err(|e| e.to_string())?;

    Ok(vec![lon, lat, elevation as f64])
}

fn add_elevation_to_coords(
    service: &htg::SrtmService,
    coords: &[Vec<f64>],
) -> Result<Vec<Vec<f64>>, String> {
    coords
        .iter()
        .map(|coord| add_elevation_to_coord(service, coord))
        .collect()
}

#[tokio::test]
async fn test_elevation_endpoint_success() {
    let temp_dir = TempDir::new().unwrap();
    create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

    let server = create_test_server(&temp_dir).await;

    let response = server.get("/elevation?lat=35.5&lon=138.5").await;

    response.assert_status_ok();
    let json: Value = response.json();
    assert_eq!(json["elevation"], 500);
    assert_eq!(json["lat"], 35.5);
    assert_eq!(json["lon"], 138.5);
}

#[tokio::test]
async fn test_elevation_endpoint_invalid_coordinates() {
    let temp_dir = TempDir::new().unwrap();
    let server = create_test_server(&temp_dir).await;

    // Latitude out of range
    let response = server.get("/elevation?lat=91.0&lon=0.0").await;
    response.assert_status(StatusCode::BAD_REQUEST);
    let json: Value = response.json();
    assert!(json["error"].as_str().unwrap().contains("out of bounds"));
}

#[tokio::test]
async fn test_elevation_endpoint_missing_tile() {
    let temp_dir = TempDir::new().unwrap();
    let server = create_test_server(&temp_dir).await;

    // No tile file exists
    let response = server.get("/elevation?lat=50.0&lon=50.0").await;
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_health_endpoint() {
    let temp_dir = TempDir::new().unwrap();
    let server = create_test_server(&temp_dir).await;

    let response = server.get("/health").await;

    response.assert_status_ok();
    let json: Value = response.json();
    assert_eq!(json["status"], "healthy");
    assert!(json["version"].as_str().is_some());
}

#[tokio::test]
async fn test_stats_endpoint() {
    let temp_dir = TempDir::new().unwrap();
    create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

    let server = create_test_server(&temp_dir).await;

    // Initial stats (no requests yet)
    let response = server.get("/stats").await;
    response.assert_status_ok();
    let json: Value = response.json();
    assert_eq!(json["cache_hits"], 0);
    assert_eq!(json["cache_misses"], 0);

    // Make a request to populate cache
    server.get("/elevation?lat=35.5&lon=138.5").await;

    // Stats should show cache miss
    let response = server.get("/stats").await;
    let json: Value = response.json();
    assert_eq!(json["cache_misses"], 1);

    // Make another request in same tile (cache hit)
    server.get("/elevation?lat=35.6&lon=138.6").await;

    let response = server.get("/stats").await;
    let json: Value = response.json();
    assert_eq!(json["cache_hits"], 1);
    assert_eq!(json["cache_misses"], 1);
}

#[tokio::test]
async fn test_elevation_endpoint_missing_params() {
    let temp_dir = TempDir::new().unwrap();
    let server = create_test_server(&temp_dir).await;

    // Missing lat parameter
    let response = server.get("/elevation?lon=138.5").await;
    response.assert_status(StatusCode::BAD_REQUEST);

    // Missing lon parameter
    let response = server.get("/elevation?lat=35.5").await;
    response.assert_status(StatusCode::BAD_REQUEST);

    // No parameters
    let response = server.get("/elevation").await;
    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_elevation_endpoint_interpolation() {
    let temp_dir = TempDir::new().unwrap();
    create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

    let server = create_test_server(&temp_dir).await;

    // Test with interpolation enabled
    let response = server
        .get("/elevation?lat=35.5&lon=138.5&interpolate=true")
        .await;

    response.assert_status_ok();
    let json: Value = response.json();

    // Should have floating-point elevation and interpolated flag
    assert!(json["elevation"].is_f64() || json["elevation"].is_i64());
    assert_eq!(json["lat"], 35.5);
    assert_eq!(json["lon"], 138.5);
    assert!(json["interpolated"].is_boolean());
}

#[tokio::test]
async fn test_elevation_endpoint_no_interpolation() {
    let temp_dir = TempDir::new().unwrap();
    create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

    let server = create_test_server(&temp_dir).await;

    // Test without interpolation (default)
    let response = server.get("/elevation?lat=35.5&lon=138.5").await;

    response.assert_status_ok();
    let json: Value = response.json();

    // Should have integer elevation and no interpolated flag
    assert_eq!(json["elevation"], 500);
    assert!(json.get("interpolated").is_none());
}

// GeoJSON POST endpoint tests

#[tokio::test]
async fn test_geojson_point() {
    let temp_dir = TempDir::new().unwrap();
    create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

    let server = create_test_server(&temp_dir).await;

    let geometry = Geometry::new(GeoJsonValue::Point(vec![138.5, 35.5]));

    let response = server.post("/elevation").json(&geometry).await;

    response.assert_status_ok();
    let json: Value = response.json();
    assert_eq!(json["type"], "Point");

    let coords = json["coordinates"].as_array().unwrap();
    assert_eq!(coords[0].as_f64().unwrap(), 138.5);
    assert_eq!(coords[1].as_f64().unwrap(), 35.5);
    assert_eq!(coords[2].as_f64().unwrap(), 500.0);
}

#[tokio::test]
async fn test_geojson_multipoint() {
    let temp_dir = TempDir::new().unwrap();
    create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

    let server = create_test_server(&temp_dir).await;

    let geometry = Geometry::new(GeoJsonValue::MultiPoint(vec![
        vec![138.5, 35.5],
        vec![138.5, 35.5],
    ]));

    let response = server.post("/elevation").json(&geometry).await;

    response.assert_status_ok();
    let json: Value = response.json();
    assert_eq!(json["type"], "MultiPoint");

    let coords = json["coordinates"].as_array().unwrap();
    assert_eq!(coords.len(), 2);

    // Both points should have elevation 500
    for coord in coords {
        let c = coord.as_array().unwrap();
        assert_eq!(c[2].as_f64().unwrap(), 500.0);
    }
}

#[tokio::test]
async fn test_geojson_linestring() {
    let temp_dir = TempDir::new().unwrap();
    create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

    let server = create_test_server(&temp_dir).await;

    let geometry = Geometry::new(GeoJsonValue::LineString(vec![
        vec![138.5, 35.5],
        vec![138.5, 35.5],
        vec![138.5, 35.5],
    ]));

    let response = server.post("/elevation").json(&geometry).await;

    response.assert_status_ok();
    let json: Value = response.json();
    assert_eq!(json["type"], "LineString");

    let coords = json["coordinates"].as_array().unwrap();
    assert_eq!(coords.len(), 3);

    // All points should have elevation 500
    for coord in coords {
        let c = coord.as_array().unwrap();
        assert_eq!(c.len(), 3); // lon, lat, elevation
        assert_eq!(c[2].as_f64().unwrap(), 500.0);
    }
}

#[tokio::test]
async fn test_geojson_multilinestring() {
    let temp_dir = TempDir::new().unwrap();
    create_test_tile(temp_dir.path(), "N35E138.hgt", 500);

    let server = create_test_server(&temp_dir).await;

    let geometry = Geometry::new(GeoJsonValue::MultiLineString(vec![
        vec![vec![138.5, 35.5], vec![138.5, 35.5]],
        vec![vec![138.5, 35.5], vec![138.5, 35.5]],
    ]));

    let response = server.post("/elevation").json(&geometry).await;

    response.assert_status_ok();
    let json: Value = response.json();
    assert_eq!(json["type"], "MultiLineString");

    let lines = json["coordinates"].as_array().unwrap();
    assert_eq!(lines.len(), 2);

    for line in lines {
        let coords = line.as_array().unwrap();
        for coord in coords {
            let c = coord.as_array().unwrap();
            assert_eq!(c[2].as_f64().unwrap(), 500.0);
        }
    }
}

#[tokio::test]
async fn test_geojson_missing_tile() {
    let temp_dir = TempDir::new().unwrap();
    let server = create_test_server(&temp_dir).await;

    // No tile exists for these coordinates
    let geometry = Geometry::new(GeoJsonValue::Point(vec![50.0, 50.0]));

    let response = server.post("/elevation").json(&geometry).await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let json: Value = response.json();
    assert!(json["error"].as_str().is_some());
}

#[tokio::test]
async fn test_geojson_invalid_coordinates() {
    let temp_dir = TempDir::new().unwrap();
    let server = create_test_server(&temp_dir).await;

    // Latitude out of range
    let geometry = Geometry::new(GeoJsonValue::Point(vec![0.0, 91.0]));

    let response = server.post("/elevation").json(&geometry).await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let json: Value = response.json();
    assert!(json["error"].as_str().unwrap().contains("out of bounds"));
}
