//! HTTP server setup — health check, metrics, and top-level router
//!
//! This module creates the main `axum::Router` that mounts:
//! - `GET /health` — liveness/readiness probe
//! - `GET /metrics` — Prometheus exposition format
//! - `/api/v1/*` — the full API router from [`crate::api`]
//! - `/swagger-ui` — interactive API documentation
//! - `/api-docs/openapi.json` — OpenAPI specification

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use prometheus::{Encoder, TextEncoder};
use serde_json::json;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::api;
use crate::api::ApiDoc;
use crate::state::AppState;

/// Build the complete HTTP router with all routes.
///
/// The router is organized as:
/// - `/health` — health check endpoint
/// - `/metrics` — Prometheus metrics endpoint
/// - `/api/v1/*` — API endpoints
/// - `/swagger-ui` — Swagger UI for API documentation
/// - `/api-docs/openapi.json` — OpenAPI JSON specification
pub fn build_http_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .nest("/api/v1", api::build_api_router())
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
}

/// Handler for `GET /health`.
///
/// Returns a JSON object indicating the node is alive, along with
/// the node ID, peer count, and finalized event count.
pub async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let node_id = state.config.node_id;
    let peers = state.peers.read().await.len();
    let finalized_height = state.event_store.read().await.len();

    axum::Json(json!({
        "status": "ok",
        "node_id": node_id,
        "peers": peers,
        "finalized_height": finalized_height,
    }))
}

/// Handler for `GET /metrics`.
///
/// Returns all registered Prometheus metrics in the standard
/// text exposition format. This endpoint is scraped by Prometheus
/// or compatible monitoring systems.
pub async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    if encoder.encode(&metric_families, &mut buffer).is_err() {
        tracing::error!("Failed to encode Prometheus metrics");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to encode metrics".to_string(),
        );
    }
    let body = String::from_utf8(buffer).unwrap_or_else(|_| {
        tracing::error!("Prometheus output contained invalid UTF-8");
        "# ERROR: invalid UTF-8 in metrics output\n".to_string()
    });
    (axum::http::StatusCode::OK, body)
}
