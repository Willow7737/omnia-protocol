//! Shard operation API handlers
//!
//! Provides the endpoint for routing operations to specific shards:
//! - `POST /api/v1/shards/:shard_id/operations` — submit an operation to a shard
//!
//! # Authorization
//!
//! Privileged operations (`mint`, `advance_epoch`) require the caller's
//! identity to appear in the [`AuthorizedCallers`] registry. Unprivileged
//! operations (`spend`, `register`) only require a valid JWT.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use omnia_shards::{EconomicsOp, ShardOp};
use omnia_substrate::{generate_keypair, Event};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::api::auth::AuthorizedCallers;
use crate::api::auth::CallerIdentity;
use crate::api::governance::ApiParseError;
use crate::state::AppState;

/// Set of operations that require elevated (privileged) authorization.
const PRIVILEGED_OPS: &[&str] = &["mint", "advance_epoch"];

/// Check whether an operation name requires privileged authorization.
fn is_privileged(op: &str) -> bool {
    PRIVILEGED_OPS.contains(&op)
}

/// Request body for submitting a shard operation.
///
/// The `operation` field determines the type of operation, and `params`
/// contains the operation-specific parameters as a JSON object.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ShardOperationRequest {
    /// Operation name (e.g., "mint", "spend", "register", "advance_epoch").
    pub operation: String,
    /// Operation-specific parameters.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub params: serde_json::Map<String, Value>,
}

/// Handler for `POST /api/v1/shards/:shard_id/operations`.
///
/// Routes the specified operation to the appropriate shard via the
/// `ShardRouter`. Currently supports operations on the economics
/// shard; other shards return a "not implemented" response.
///
/// Privileged operations (`mint`, `advance_epoch`) require the caller
/// to be listed in the [`AuthorizedCallers`] registry. Unprivileged
/// operations only require a valid JWT (enforced by the `require_auth`
/// middleware).
#[utoipa::path(
    post,
    path = "/api/v1/shards/{shard_id}/operations",
    request_body = ShardOperationRequest,
    params(
        ("shard_id" = String, Path, description = "Shard identifier")
    ),
    responses(
        (status = 200, description = "Operation processed"),
        (status = 400, description = "Invalid request"),
        (status = 403, description = "Forbidden — caller not authorized for privileged operation"),
        (status = 404, description = "Unknown shard"),
    )
)]
pub async fn submit_shard_operation(
    State(state): State<AppState>,
    Path(shard_id): Path<String>,
    Extension(authorized): Extension<Arc<AuthorizedCallers>>,
    Extension(caller): Extension<CallerIdentity>,
    Json(body): Json<ShardOperationRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    tracing::info!(
        shard_id = %shard_id,
        operation = %body.operation,
        caller = %caller.caller_id,
        "Processing shard operation"
    );

    // Check authorization for privileged operations
    if is_privileged(&body.operation) && !authorized.is_authorized(&caller.caller_id) {
        tracing::warn!(
            operation = %body.operation,
            caller = %caller.caller_id,
            "Unauthorized privileged operation attempt"
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": format!("caller '{}' is not authorized for privileged operation '{}'",
                    caller.caller_id, body.operation),
            })),
        ));
    }

    match shard_id.as_str() {
        "economics" => handle_economics_op(&state, &body).await,
        "financial" => handle_generic_shard_op(&state, &shard_id, &body).await,
        "computational" => handle_generic_shard_op(&state, &shard_id, &body).await,
        "physical" => handle_generic_shard_op(&state, &shard_id, &body).await,
        "biological" => handle_generic_shard_op(&state, &shard_id, &body).await,
        "identity" => handle_generic_shard_op(&state, &shard_id, &body).await,
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("Unknown shard: {}", shard_id)})),
        )),
    }
}

/// Handle an economics shard operation.
///
/// Parses the operation name and parameters, creates an `EconomicsOp`,
/// and routes it through the shard router.
async fn handle_economics_op(
    state: &AppState,
    body: &ShardOperationRequest,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let econ_op = parse_economics_op(body)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;

    let node_id = state.config.node_id_bytes();
    let keypair = generate_keypair();
    let mut event = Event::genesis(node_id, Vec::new());
    event.sign_with_keypair(&keypair);

    let shard_op = ShardOp::Economics(econ_op);

    let mut router = state.shard_router.lock().await;
    match router.route(&event, shard_op) {
        Ok(()) => {
            tracing::info!(operation = %body.operation, "Economics operation processed successfully");
            Ok((
                StatusCode::OK,
                Json(json!({
                    "status": "processed",
                    "shard_id": "economics",
                    "operation": body.operation,
                })),
            ))
        }
        Err(e) => {
            tracing::warn!(operation = %body.operation, error = %e, "Economics operation failed");
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Operation failed: {}", e),
                    "shard_id": "economics",
                    "operation": body.operation,
                })),
            ))
        }
    }
}

/// Handle a generic (non-economics) shard operation.
///
/// Returns a "not implemented" response for shards that don't yet
/// have API-level operation support.
async fn handle_generic_shard_op(
    _state: &AppState,
    shard_id: &str,
    body: &ShardOperationRequest,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    tracing::info!(
        shard_id = %shard_id,
        operation = %body.operation,
        "Generic shard operation — not yet fully implemented"
    );
    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "accepted",
            "shard_id": shard_id,
            "operation": body.operation,
            "note": "Shard operation accepted but full routing not yet implemented for this shard type",
        })),
    ))
}

/// Parse an economics operation from the request body.
///
/// Supported operations:
/// - `mint` — Mint UBC to a DID (params: `did`, `amount`) **[privileged]**
/// - `spend` — Spend UBC from a DID (params: `did`, `amount`)
/// - `register` — Register a DID in the quota system (params: `did`)
/// - `advance_epoch` — Advance to the next epoch **[privileged]**
fn parse_economics_op(body: &ShardOperationRequest) -> Result<EconomicsOp, ApiParseError> {
    match body.operation.as_str() {
        "mint" => {
            let did = body
                .params
                .get("did")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ApiParseError::MissingParameter("did".to_string()))?
                .to_string();
            let amount = body
                .params
                .get("amount")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| ApiParseError::InvalidParameter("amount".to_string()))?;
            Ok(EconomicsOp::MintUbc { did, amount })
        }
        "spend" => {
            let did = body
                .params
                .get("did")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ApiParseError::MissingParameter("did".to_string()))?
                .to_string();
            let amount = body
                .params
                .get("amount")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| ApiParseError::InvalidParameter("amount".to_string()))?;
            Ok(EconomicsOp::SpendUbc { did, amount })
        }
        "register" => {
            let did = body
                .params
                .get("did")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ApiParseError::MissingParameter("did".to_string()))?
                .to_string();
            Ok(EconomicsOp::RegisterDid { did })
        }
        "advance_epoch" => Ok(EconomicsOp::AdvanceEpoch),
        other => Err(ApiParseError::UnknownOperation(other.to_string())),
    }
}
