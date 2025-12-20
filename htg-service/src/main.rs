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
use htg::SrtmServiceBuilder;
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
