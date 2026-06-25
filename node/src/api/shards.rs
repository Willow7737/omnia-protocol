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
use omnia_shards::EconomicsOp;
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
            Json(json!({"error": format!("Unknown shard: {shard_id}")})),
        )),
    }
}

/// Handle an economics shard operation.
///
/// Parses the operation name and parameters, creates an `EconomicsOp`,
/// and applies it DIRECTLY to the shared `AppState.economics` state
/// instance.
///
/// SECURITY FIX (audit): The previous implementation routed economics
/// operations through `ShardRouter::route()`, which dispatched to the
/// `EconomicsShard`'s INTERNAL `EconomicsState` instance — a DIFFERENT
/// instance from `AppState.economics` used by the `/balance`, `/transfer`,
/// and `/governance` endpoints. Mints via this endpoint were invisible to
/// balance reads, breaking the economics model. We now apply operations
/// directly to the shared instance so all endpoints see the same state.
///
/// TODO: For full multi-node propagation, economics operations should be
/// submitted as events through the substrate (consensus + gossip), not
/// applied locally. The current implementation still operates locally —
/// other nodes will not see these mutations until the substrate-event
/// submission path is wired in.
async fn handle_economics_op(
    state: &AppState,
    body: &ShardOperationRequest,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let econ_op = parse_economics_op(body).map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;

    // Apply the operation directly to the shared AppState.economics
    // instance. This fixes the dual-state disconnect.
    //
    // P0-2 security fix: pass the node's public key as event_creator
    // instead of None. The node signs events on behalf of authenticated
    // API callers, so the node's pubkey is the correct "creator" for
    // economics operations submitted via the HTTP API. This closes the
    // auth bypass where None was treated as "testing mode" with weaker
    // authorization (admin gating only, no DID ownership verification).
    let caller_pubkey = state.keypair.as_ref().map(|kp| kp.verifying_key().to_bytes());

    let mut econ_state = state.economics.lock().await;
    let current_epoch = econ_state.current_epoch();
    match econ_state.apply(&econ_op, current_epoch, caller_pubkey.as_ref()) {
        Ok(()) => {
            tracing::info!(operation = %body.operation, "Economics operation processed successfully");
            Ok((
                StatusCode::OK,
                Json(json!({
                    "status": "processed",
                    "shard_id": "economics",
                    "operation": body.operation,
                    "note": "applied to local state only — multi-node propagation not yet wired",
                })),
            ))
        }
        Err(e) => {
            tracing::warn!(operation = %body.operation, error = %e, "Economics operation failed");
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Operation failed: {e}"),
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
    Err((
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "status": "not_implemented",
            "shard_id": shard_id,
            "operation": body.operation,
            "message": format!("Shard '{}' operations are not yet fully implemented. Use the economics shard for active operations.", shard_id),
        })),
    ))
}

/// Parse an economics operation from the request body.
///
/// Supported operations:
/// - `mint` — Mint UBC to a DID (params: `did`, `amount`) **\[privileged\]**
/// - `spend` — Spend UBC from a DID (params: `did`, `amount`)
/// - `register` — Register a DID in the quota system (params: `did`)
/// - `advance_epoch` — Advance to the next epoch **\[privileged\]**
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
