//! Economics balance and transfer API handlers
//!
//! Provides endpoints for querying UBC balances and transferring
//! (spending) UBC tokens:
//! - `GET /api/v1/economics/balance/:did` — check UBC balance
//! - `POST /api/v1/economics/transfer` — spend/transfer UBC

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

/// Request body for a UBC transfer (spend) operation.
///
/// Note: UBC tokens are **soulbound** — they cannot be transferred
/// between identities. This endpoint performs a *spend* operation
/// that consumes UBC from the sender's balance. The `to_did` field
/// is accepted for API compatibility but UBC is not actually
/// transferred to the recipient.
#[derive(Debug, Clone, Deserialize)]
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
                "error": format!("DID not registered: {}", did),
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
pub async fn transfer_ubc(
    State(state): State<AppState>,
    Json(body): Json<TransferRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if body.amount == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Transfer amount must be greater than zero"})),
        ));
    }

    let mut economics = state.economics.lock().await;

    // Ensure the sender is registered
    if !economics.quota.is_registered(&body.from_did) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": format!("Sender DID not registered: {}", body.from_did),
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
    let result = economics.quota.spend(&body.from_did, body.amount);

    match result {
        Ok(()) => {
            let new_balance = economics.balance_of(&body.from_did).unwrap_or(0);
            tracing::info!(
                from_did = %body.from_did,
                to_did = %body.to_did,
                amount = body.amount,
                new_balance = new_balance,
                "UBC spend operation completed"
            );
            Ok((
                StatusCode::OK,
                Json(json!({
                    "status": "completed",
                    "from_did": body.from_did,
                    "to_did": body.to_did,
                    "amount": body.amount,
                    "new_balance": new_balance,
                    "note": "UBC is soulbound — tokens are spent (consumed), not transferred to the recipient",
                })),
            ))
        }
        Err(e) => {
            tracing::warn!(
                from_did = %body.from_did,
                amount = body.amount,
                error = %e,
                "UBC spend operation failed"
            );
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Transfer failed: {}", e),
                    "from_did": body.from_did,
                    "amount": body.amount,
                })),
            ))
        }
    }
}
