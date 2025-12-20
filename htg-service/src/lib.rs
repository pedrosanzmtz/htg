//! HTG Service Library
//!
//! HTTP handlers and types for the SRTM elevation service.
//! This library is used by both the htg-service binary and integration tests.

pub mod handlers;

use htg::SrtmService;

/// Application state shared across handlers.
pub struct AppState {
    /// SRTM service for elevation queries.
    pub srtm_service: SrtmService,
}

// Re-export commonly used types for convenience
pub use handlers::{
    ElevationQuery, ElevationResponse, ErrorResponse, HealthResponse,
    InterpolatedElevationResponse, StatsResponse,
};
