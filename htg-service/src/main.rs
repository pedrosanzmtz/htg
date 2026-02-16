//! HTG Service - HTTP microservice for SRTM elevation queries.
//!
//! A high-performance REST API for querying elevation data from SRTM files.
//!
//! ## Environment Variables
//!
//! | Variable | Description | Default |
//! |----------|-------------|---------|
//! | `HTG_DATA_DIR` | Directory containing .hgt files | Required |
//! | `HTG_CACHE_SIZE` | Maximum tiles in cache | 100 |
//! | `HTG_PORT` | HTTP server port | 8080 |
//! | `HTG_DOWNLOAD_SOURCE` | Named source: "ardupilot", "ardupilot-srtm1", "ardupilot-srtm3" | None |
//! | `HTG_DOWNLOAD_URL` | URL template for auto-download | None |
//! | `HTG_DOWNLOAD_GZIP` | Whether downloads are gzipped | false |
//! | `RUST_LOG` | Log level (e.g., "info", "debug") | "info" |
//!
//! ## Endpoints
//!
//! - `GET /elevation?lat=X&lon=Y` - Get elevation at coordinates
//! - `POST /elevation` - Batch elevation query with GeoJSON geometry
//! - `GET /health` - Health check
//! - `GET /stats` - Cache statistics
//! - `GET /docs` - OpenAPI documentation (Swagger UI)

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{routing::get, Router};
use htg::{BoundingBox, SrtmServiceBuilder};
use htg_service::{handlers, AppState};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// OpenAPI documentation for the HTG service.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "HTG Elevation Service",
        version = "0.1.0",
        description = "High-performance REST API for querying elevation data from SRTM files.",
        license(name = "MIT", url = "https://opensource.org/licenses/MIT"),
        contact(name = "Pedro Sanz Martinez", url = "https://github.com/pedrosanzmtz/htg")
    ),
    paths(
        handlers::get_elevation,
        handlers::post_elevation,
        handlers::health_check,
        handlers::get_stats,
    ),
    components(
        schemas(
            handlers::ElevationQuery,
            handlers::ElevationResponse,
            handlers::InterpolatedElevationResponse,
            handlers::ErrorResponse,
            handlers::HealthResponse,
            handlers::StatsResponse,
        )
    ),
    tags(
        (name = "elevation", description = "Elevation query endpoints"),
        (name = "system", description = "System and health endpoints")
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "htg_service=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load port from environment (service-specific config)
    let port: u16 = std::env::var("HTG_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    // Build SRTM service from environment variables using the library
    // The library handles: HTG_DATA_DIR, HTG_CACHE_SIZE, HTG_DOWNLOAD_SOURCE,
    // HTG_DOWNLOAD_URL, HTG_DOWNLOAD_GZIP
    let srtm_service = match SrtmServiceBuilder::from_env() {
        Ok(builder) => builder.build()?,
        Err(_) => {
            // Fallback: HTG_DATA_DIR not set, use current directory
            tracing::warn!("HTG_DATA_DIR not set, using current directory");
            SrtmServiceBuilder::new(".").build()?
        }
    };

    tracing::info!(
        data_dir = %srtm_service.data_dir().display(),
        cache_capacity = srtm_service.cache_capacity(),
        auto_download = srtm_service.has_auto_download(),
        port = port,
        "Starting HTG service"
    );

    // Handle HTG_PRELOAD environment variable
    if let Ok(preload_val) = std::env::var("HTG_PRELOAD") {
        let bounds = parse_preload_bounds(&preload_val);
        let bounds_ref = bounds.as_deref();
        tracing::info!(
            bounds = ?bounds_ref.map(|b| b.len()),
            "Preloading tiles into cache"
        );
        let stats = srtm_service.preload(bounds_ref);
        tracing::info!(
            tiles_loaded = stats.tiles_loaded,
            tiles_already_cached = stats.tiles_already_cached,
            tiles_failed = stats.tiles_failed,
            tiles_matched = stats.tiles_matched,
            elapsed_ms = stats.elapsed_ms,
            "Preload complete"
        );
    }

    let state = Arc::new(AppState { srtm_service });

    // Build router
    let app = Router::new()
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .route(
            "/elevation",
            get(handlers::get_elevation).post(handlers::post_elevation),
        )
        .route("/health", get(handlers::health_check))
        .route("/stats", get(handlers::get_stats))
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("Listening on http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Parse the `HTG_PRELOAD` environment variable value into bounding boxes.
///
/// Supported formats:
/// - `true`, `all`, `1` — preload all tiles (returns `None`)
/// - `min_lat,min_lon,max_lat,max_lon` — single bounding box
/// - `min_lat,min_lon,max_lat,max_lon;min_lat,min_lon,max_lat,max_lon` — multiple bounding boxes
fn parse_preload_bounds(value: &str) -> Option<Vec<BoundingBox>> {
    let trimmed = value.trim();

    // Check for "all tiles" keywords
    match trimmed.to_lowercase().as_str() {
        "true" | "all" | "1" => return None,
        _ => {}
    }

    // Parse as bounding boxes separated by ';'
    let boxes: Vec<BoundingBox> = trimmed
        .split(';')
        .filter_map(|bbox_str| {
            let parts: Vec<f64> = bbox_str
                .split(',')
                .filter_map(|s| s.trim().parse::<f64>().ok())
                .collect();
            if parts.len() == 4 {
                Some(BoundingBox::new(parts[0], parts[1], parts[2], parts[3]))
            } else {
                tracing::warn!(
                    bbox = bbox_str,
                    "Invalid bounding box format, expected min_lat,min_lon,max_lat,max_lon"
                );
                None
            }
        })
        .collect();

    if boxes.is_empty() {
        // If parsing failed entirely, fall back to loading all tiles
        tracing::warn!(
            value = trimmed,
            "Could not parse HTG_PRELOAD value, preloading all tiles"
        );
        None
    } else {
        Some(boxes)
    }
}
