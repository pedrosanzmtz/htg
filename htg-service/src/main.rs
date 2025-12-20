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
//! | `HTG_DOWNLOAD_URL` | URL template for auto-download | None |
//! | `HTG_DOWNLOAD_GZIP` | Whether downloads are gzipped | false |
//! | `RUST_LOG` | Log level (e.g., "info", "debug") | "info" |
//!
//! ## Endpoints
//!
//! - `GET /elevation?lat=X&lon=Y` - Get elevation at coordinates
//! - `GET /health` - Health check
//! - `GET /stats` - Cache statistics

mod handlers;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{routing::get, Router};
use htg::SrtmService;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Application state shared across handlers.
pub struct AppState {
    /// SRTM service for elevation queries.
    pub srtm_service: SrtmService,
}

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

    // Load configuration from environment
    let data_dir = std::env::var("HTG_DATA_DIR").unwrap_or_else(|_| {
        tracing::warn!("HTG_DATA_DIR not set, using current directory");
        ".".to_string()
    });

    let cache_size: u64 = std::env::var("HTG_CACHE_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let port: u16 = std::env::var("HTG_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    tracing::info!(
        data_dir = %data_dir,
        cache_size = cache_size,
        port = port,
        "Starting HTG service"
    );

    // Build SRTM service
    let srtm_service = build_srtm_service(&data_dir, cache_size)?;

    let state = Arc::new(AppState { srtm_service });

    // Build router
    let app = Router::new()
        .route("/elevation", get(handlers::get_elevation))
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

/// Build the SRTM service from environment configuration.
fn build_srtm_service(
    data_dir: &str,
    cache_size: u64,
) -> Result<SrtmService, Box<dyn std::error::Error>> {
    let mut builder = htg::SrtmServiceBuilder::new(data_dir).cache_size(cache_size);

    // Check for download configuration
    if let Ok(url_template) = std::env::var("HTG_DOWNLOAD_URL") {
        let is_gzipped = std::env::var("HTG_DOWNLOAD_GZIP")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        tracing::info!(
            url_template = %url_template,
            is_gzipped = is_gzipped,
            "Auto-download enabled"
        );

        builder = builder.auto_download(htg::download::DownloadConfig::with_url_template(
            url_template,
            is_gzipped,
        ));
    }

    Ok(builder.build()?)
}
