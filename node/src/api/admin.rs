//! Admin-only endpoints for manual operational actions.
//!
//! These endpoints allow operators to manually trigger settlement
//! submissions and other maintenance tasks that are not yet automated
//! (see ADR-025 gap: no specified trigger mechanism for submissions).

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Request body for submitting a state root to the settlement layer.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SubmitRootRequest {
    /// Hex-encoded 32-byte state root (with or without `0x` prefix).
    #[schema(example = "0x0000000000000000000000000000000000000000000000000000000000000000")]
    pub root: String,
}

/// Response body for a successful root submission.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SubmitRootResponse {
    /// Transaction hash of the on-chain anchor transaction.
    pub tx_hash: String,
}

/// Manually submit a state root to the configured settlement adapter.
///
/// Returns the on-chain transaction hash on success. This is the
/// admin-triggered path recommended in the ADR-025 gap analysis:
/// instead of designing an automatic submission trigger, an operator
/// calls this endpoint to push a root to L1.
///
/// # Errors
///
/// - `400` — root is not valid hex or not 32 bytes
/// - `503` — settlement adapter is not live (mock/stub)
/// - `500` — settlement submission failed on L1
#[utoipa::path(
    post,
    path = "/admin/settlement/submit-root",
    request_body = SubmitRootRequest,
    responses(
        (status = 200, description = "Root submitted successfully", body = SubmitRootResponse),
        (status = 400, description = "Invalid root hex"),
        (status = 503, description = "Settlement adapter is not live"),
        (status = 500, description = "L1 submission failed"),
    ),
    tag = "Admin"
)]
pub async fn submit_root(
    State(state): State<AppState>,
    Json(req): Json<SubmitRootRequest>,
) -> Result<(StatusCode, Json<SubmitRootResponse>), (StatusCode, Json<serde_json::Value>)> {
    // Refuse to submit if the adapter isn't connected to a real L1.
    if !state.settlement.is_live() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "settlement adapter is not live — no real L1 connection. \
                          Enable --features bitcoin-live or --features ethereum-live \
                          and configure the corresponding env vars."
            })),
        ));
    }

    // Parse hex root (accept "0x..." or bare hex).
    let root_str = req.root.strip_prefix("0x").unwrap_or(&req.root);
    let root_bytes = match hex::decode(root_str) {
        Ok(b) => b,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid hex in root: {e}")
                })),
            ));
        }
    };
    if root_bytes.len() != 32 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("root must be 32 bytes, got {}", root_bytes.len())
            })),
        ));
    }
    let mut root = [0u8; 32];
    root.copy_from_slice(&root_bytes);

    // Call the settlement adapter's submit_root.
    match state.settlement.submit_root(root).await {
        Ok(tx_hash) => Ok((
            StatusCode::OK,
            Json(SubmitRootResponse {
                tx_hash: tx_hash.to_string(),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("settlement submission failed: {e}")
            })),
        )),
    }
}
