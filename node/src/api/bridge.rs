//! Bridge API handlers — Spec §8, §15
//!
//! Endpoints:
//! - `GET  /api/v1/bridge/providers`     — list registered bridge providers
//! - `GET  /api/v1/bridge/health`        — health check for all providers

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;

/// Handler for `GET /api/v1/bridge/providers`.
///
/// Lists the supported Ghana mobile-money providers. Per Spec §8.1.
#[utoipa::path(
    get,
    path = "/api/v1/bridge/providers",
    responses(
        (status = 200, description = "List of bridge providers"),
    )
)]
pub async fn list_providers(
    State(_state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let providers = json!([
        {
            "id": "MTN",
            "label": "MTN Mobile Money",
            "ussd": "*170#",
            "status": "active",
        },
        {
            "id": "Telecel",
            "label": "Telecel Cash",
            "ussd": "*110#",
            "status": "active",
        },
        {
            "id": "AT",
            "label": "AT Money",
            "ussd": "*505#",
            "status": "active",
        },
    ]);

    Ok((StatusCode::OK, Json(json!({ "providers": providers }))))
}

/// Handler for `GET /api/v1/bridge/health`.
///
/// Returns the health status of the bridge subsystem.
/// Per Spec §15 circuit breaker.
#[utoipa::path(
    get,
    path = "/api/v1/bridge/health",
    responses(
        (status = 200, description = "Bridge subsystem health"),
    )
)]
pub async fn bridge_health(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let treasury = state.treasury.read().await;
    let cb = treasury.circuit_breaker();

    Ok((
        StatusCode::OK,
        Json(json!({
            "status": if cb.paused { "degraded" } else { "healthy" },
            "circuit_breaker_paused": cb.paused,
        })),
    ))
}