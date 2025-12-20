//! Integration tests for the HTTP API.

use axum::{routing::get, Router};
use axum_test::TestServer;
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
        .route("/elevation", get(get_elevation))
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
}

#[derive(Debug, Serialize)]
pub struct ElevationResponse {
    pub elevation: i16,
    pub lat: f64,
    pub lon: f64,
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
        Err(e) => {
            let (status, message) = match &e {
                htg::SrtmError::OutOfBounds { .. } => (StatusCode::BAD_REQUEST, e.to_string()),
                htg::SrtmError::FileNotFound { .. } | htg::SrtmError::TileNotAvailable { .. } => {
                    (StatusCode::NOT_FOUND, e.to_string())
                }
                _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            };
            (status, Json(ErrorResponse { error: message })).into_response()
        }
    }
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
