//! HTTP server setup — health check, metrics, CORS, rate limiting, and top-level router
//!
//! This module creates the main `axum::Router` that mounts:
//! - `GET /health` — liveness/readiness probe
//! - `GET /metrics` — Prometheus exposition format
//! - `/api/v1/*` — the full API router from [`crate::api`]
//! - `/swagger-ui` — interactive API documentation
//! - `/api-docs/openapi.json` — OpenAPI specification
//!
//! # Middleware (outermost first)
//!
//! 1. **CORS** — permissive defaults for development; tighten for production
//! 2. **Rate limiting** — per-client token-bucket rate limiter

use std::sync::Arc;

use axum::extract::State;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Extension;
use axum::Router;
use prometheus::{Encoder, TextEncoder};
use serde_json::json;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::api;
use crate::api::auth::{self as api_auth, default_cors_layer, RateLimiter};
use crate::api::ApiDoc;
use crate::state::AppState;

/// Build the complete HTTP router with all routes, CORS, and rate limiting.
///
/// The router is organized as:
/// - `/health` — health check endpoint
/// - `/metrics` — Prometheus metrics endpoint
/// - `/api/v1/*` — API endpoints (with JWT auth and authorization)
/// - `/swagger-ui` — Swagger UI for API documentation
/// - `/api-docs/openapi.json` — OpenAPI JSON specification
///
/// # Middleware
///
/// - **CORS** — allows all origins, standard methods and headers
/// - **Rate limiting** — per-client token-bucket (configurable via `OMNIA_RATE_LIMIT_RPS`)
///
/// # Layer ordering
///
/// Layers are applied in reverse order (last added = outermost):
///
/// ```text
/// Request → CORS → RateLimit → Extension(inject) → Handler
/// ```
///
/// The `Extension` layer must be innermost so it injects the
/// [`RateLimiter`] into request extensions *before* the rate-limit
/// middleware attempts to extract it.
pub fn build_http_router() -> Router<AppState> {
    let rate_limiter = Arc::new(RateLimiter::from_env());
    let cors = default_cors_layer();

    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .nest("/api/v1", api::build_api_router())
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        // --- Middleware layers ---
        // Order: last added = outermost.
        // Request flow: CORS → Extension(inject) → RateLimit → Handler
        .layer(middleware::from_fn(api_auth::rate_limit_middleware))
        .layer(Extension(rate_limiter))
        .layer(cors)
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
