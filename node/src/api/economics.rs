//! Economics balance and transfer API handlers
//!
//! Provides endpoints for querying UBC balances and transferring
//! (spending) UBC tokens:
//! - `GET /api/v1/economics/balance/:did` — check UBC balance
//! - `POST /api/v1/economics/transfer` — spend/transfer UBC
//! - `GET /api/v1/economics/transfers` — list recent transfer records

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::api::auth::CallerIdentity;
use crate::state::{AppState, TransferRecord};

/// Request body for a UBC transfer (spend) operation.
///
/// Note: UBC tokens are **soulbound** — they cannot be transferred
/// between identities. This endpoint performs a *spend* operation
/// that consumes UBC from the sender's balance. The `to_did` field
/// is accepted for API compatibility but UBC is not actually
/// transferred to the recipient.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TransferRequest {
    /// DID sending (spending) UBC.
    pub from_did: String,
    /// DID of the intended recipient (informational only — UBC is soulbound).
    pub to_did: String,
    /// Amount of UBC to spend.
    pub amount: u64,
}

/// Handler for `GET /api/v1/economics/balance/:did`.
///
/// Returns the current UBC balance and monthly quota for the
/// specified DID. Returns 404 if the DID is not registered.
#[utoipa::path(
    get,
    path = "/api/v1/economics/balance/{did}",
    params(
        ("did" = String, Path, description = "Decentralized identifier")
    ),
    responses(
        (status = 200, description = "Balance found"),
        (status = 404, description = "DID not registered"),
    )
)]
pub async fn get_balance(
    State(state): State<AppState>,
    Path(did): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let economics = state.economics.lock().await;

    let balance = economics.balance_of(&did);
    let quota = economics.quota.quota_of(&did);
    let is_registered = economics.quota.is_registered(&did);

    if !is_registered {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": format!("DID not registered: {did}"),
                "did": did,
            })),
        ));
    }

    Ok(Json(json!({
        "did": did,
        "balance": balance.unwrap_or(0),
        "monthly_quota": quota.unwrap_or(0),
        "current_epoch": economics.current_epoch(),
        "is_registered": is_registered,
    })))
}

/// Handler for `POST /api/v1/economics/transfer`.
///
/// Performs a UBC spend operation from the sender's balance.
/// Since UBC is soulbound (non-transferable), the tokens are
/// consumed rather than transferred to the recipient.
#[utoipa::path(
    post,
    path = "/api/v1/economics/transfer",
    request_body = TransferRequest,
    responses(
        (status = 200, description = "Transfer completed"),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "DID not registered"),
    )
)]
pub async fn transfer_ubc(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
    Json(body): Json<TransferRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if body.amount == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Transfer amount must be greater than zero"})),
        ));
    }

    // SECURITY: Derive from_did from the authenticated caller's identity
    // instead of trusting the request body. This prevents authorization
    // bypass where any user could spend any other user's UBC.
    let from_did = caller.caller_id;

    // Warn if the body's from_did doesn't match the authenticated identity
    if body.from_did != from_did {
        tracing::warn!(
            authenticated_did = %from_did,
            requested_did = %body.from_did,
            "Transfer from_did in request body does not match authenticated caller — using authenticated identity"
        );
    }

    let mut economics = state.economics.lock().await;

    // Ensure the sender (authenticated caller) is registered
    if !economics.quota.is_registered(&from_did) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": format!("Sender DID not registered: {}", from_did),
            })),
        ));
    }

    // Ensure the recipient is registered
    if !economics.quota.is_registered(&body.to_did) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": format!("Recipient DID not registered: {}", body.to_did),
            })),
        ));
    }

    // Perform the spend operation (UBC is soulbound — no actual transfer)
    let result = economics.quota.spend(&from_did, body.amount);

    match result {
        Ok(()) => {
            let new_balance = economics.balance_of(&from_did).unwrap_or(0);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            // Derive a stable transfer ID: BLAKE3 of (from_did, to_did, amount, timestamp).
            // Using BLAKE3 (rather than a UUID) keeps IDs deterministic and
            // auditable — anyone with the same inputs can recompute the ID.
            let id_input = format!("{from_did}|{}|{}|{now_ms}", body.to_did, body.amount);
            let id = blake3::hash(id_input.as_bytes()).to_hex().to_string();

            let record = TransferRecord {
                id: id.clone(),
                from_did: from_did.clone(),
                to_did: body.to_did.clone(),
                amount: body.amount,
                timestamp: now_ms,
                status: "completed".to_string(),
                new_balance,
            };

            // Release the economics lock before acquiring the history lock
            // to avoid lock-ordering surprises across endpoints.
            drop(economics);
            crate::state::record_transfer(&state.transfer_history, record).await;

            tracing::info!(
                from_did = %from_did,
                to_did = %body.to_did,
                amount = body.amount,
                new_balance = new_balance,
                transfer_id = %id,
                "UBC spend operation completed"
            );
            Ok((
                StatusCode::OK,
                Json(json!({
                    "status": "completed",
                    "id": id,
                    "from_did": from_did,
                    "to_did": body.to_did,
                    "amount": body.amount,
                    "new_balance": new_balance,
                    "note": "UBC is soulbound — tokens are spent (consumed), not transferred to the recipient",
                })),
            ))
        }
        Err(e) => {
            tracing::warn!(
                from_did = %from_did,
                amount = body.amount,
                error = %e,
                "UBC spend operation failed"
            );
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Transfer failed: {e}"),
                    "from_did": from_did,
                    "amount": body.amount,
                })),
            ))
        }
    }
}

/// Query parameters for `GET /api/v1/economics/transfers`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListTransfersQuery {
    /// Maximum number of records to return (default: 50, max: 1000).
    #[serde(default = "default_transfer_limit")]
    pub limit: usize,
}

fn default_transfer_limit() -> usize {
    50
}

/// Maximum number of transfer records to return per request.
const MAX_TRANSFER_LIST_LIMIT: usize = 1000;

/// Handler for `GET /api/v1/economics/transfers`.
///
/// Returns the most recent UBC spend operations, newest first.
/// Failed transfers are not recorded in the history — only successful
/// spends appear here.
#[utoipa::path(
    get,
    path = "/api/v1/economics/transfers",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum number of records to return (default 50, max 1000)")
    ),
    responses(
        (status = 200, description = "List of transfer records"),
    )
)]
pub async fn list_transfers(State(state): State<AppState>, Query(query): Query<ListTransfersQuery>) -> Json<Value> {
    let limit = query.limit.min(MAX_TRANSFER_LIST_LIMIT);
    let history = state.transfer_history.read().await;

    // Newest first — Vec's natural order is oldest first.
    let transfers: Vec<&TransferRecord> = history.iter().rev().take(limit).collect();

    Json(json!({
        "transfers": transfers,
        "count": transfers.len(),
        "total_in_history": history.len(),
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_request_deserializes_valid() {
        let json = r#"{"from_did":"did:omnia:abc","to_did":"did:omnia:def","amount":100}"#;
        let req: TransferRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.from_did, "did:omnia:abc");
        assert_eq!(req.to_did, "did:omnia:def");
        assert_eq!(req.amount, 100);
    }

    #[test]
    fn test_transfer_request_rejects_missing_amount() {
        let json = r#"{"from_did":"did:omnia:abc","to_did":"did:omnia:def"}"#;
        let result: Result<TransferRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_transfer_request_rejects_missing_from_did() {
        let json = r#"{"to_did":"did:omnia:def","amount":100}"#;
        let result: Result<TransferRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_transfer_request_rejects_missing_to_did() {
        let json = r#"{"from_did":"did:omnia:abc","amount":100}"#;
        let result: Result<TransferRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_transfer_request_accepts_zero_amount() {
        // The handler rejects amount==0 at runtime, but deserialization
        // itself should succeed — the validation is in the handler, not
        // the type. This test documents that boundary.
        let json = r#"{"from_did":"a","to_did":"b","amount":0}"#;
        let req: TransferRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.amount, 0);
    }

    #[test]
    fn test_transfer_request_accepts_large_amount() {
        let json = r#"{"from_did":"a","to_did":"b","amount":18446744073709551615}"#;
        let req: TransferRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.amount, u64::MAX);
    }

    #[test]
    fn test_transfer_request_rejects_negative_amount() {
        // u64 can't be negative, but serde_json should reject a negative literal
        let json = r#"{"from_did":"a","to_did":"b","amount":-1}"#;
        let result: Result<TransferRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_transfer_request_rejects_non_numeric_amount() {
        let json = r#"{"from_did":"a","to_did":"b","amount":"lots"}"#;
        let result: Result<TransferRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
