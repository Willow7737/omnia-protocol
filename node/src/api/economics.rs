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
use crate::api::events::build_sign_submit_event;
use crate::state::{AppState, TransferRecord};

/// Domain-tagged wire prefix for transfer-provenance event payloads.
///
/// Guarantees a transfer receipt can never be mistaken for a shard
/// operation and is self-identifying to future decoders.
const TRANSFER_PAYLOAD_TAG: &[u8] = b"OMNIA_XFER_V1";

/// On-chain provenance record for a UBC transfer, embedded (tagged +
/// postcard-encoded) in the payload of a causal-graph event.
///
/// This is the Step-1 provenance carrier (ADR-025): every transfer becomes
/// a signed, Lane-0-finalized, gossiped DAG event. The authoritative
/// balance change still happens in the economics state — event-sourced
/// balance application across nodes is the follow-on that requires sharing
/// the economics state between the API and the shard router.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TransferEventPayload {
    from_did: String,
    to_did: String,
    amount: u64,
    transfer_id: String,
    timestamp: u64,
}

/// Encode a transfer receipt as `TAG ++ postcard(payload)`.
fn encode_transfer_payload(p: &TransferEventPayload) -> Vec<u8> {
    let mut bytes = TRANSFER_PAYLOAD_TAG.to_vec();
    // postcard encoding of a small struct is infallible in practice; on the
    // impossible error, fall back to just the tag (an empty-body receipt).
    if let Ok(body) = postcard::to_allocvec(p) {
        bytes.extend(body);
    }
    bytes
}

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

    // Reject self-transfers. UBC is soulbound: a "transfer" spends (burns)
    // the sender's tokens without crediting the recipient, so sending to
    // yourself can only destroy your own balance. Fail loudly instead of
    // letting callers burn tokens by accident.
    if body.to_did == from_did {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Cannot transfer to yourself: UBC is soulbound, so a transfer burns the sender's tokens without crediting the recipient. A self-transfer would only destroy your balance.",
                "from_did": from_did,
            })),
        ));
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

            // Release the economics lock BEFORE taking the substrate write
            // lock (event submission) or the history lock, to keep a single
            // consistent lock order across endpoints.
            drop(economics);

            // Record the transfer on-chain as a signed causal-graph event:
            // provenance + Lane 0 fast-path finality + gossip propagation.
            // The authoritative balance change already happened above; if
            // the provenance event fails to submit, the transfer still
            // succeeded — we just record event_id: None and log it.
            let payload = encode_transfer_payload(&TransferEventPayload {
                from_did: from_did.clone(),
                to_did: body.to_did.clone(),
                amount: body.amount,
                transfer_id: id.clone(),
                timestamp: now_ms,
            });
            let event_id = match build_sign_submit_event(&state, payload).await {
                Ok(submitted) => match submitted.submit_result {
                    Ok(()) => Some(submitted.event_id_hex),
                    Err(e) => {
                        tracing::warn!(
                            transfer_id = %id,
                            error = %e,
                            "Transfer succeeded but provenance event was not accepted by the substrate"
                        );
                        None
                    }
                },
                Err((_status, body)) => {
                    tracing::warn!(
                        transfer_id = %id,
                        error = %body.0,
                        "Transfer succeeded but provenance event could not be built"
                    );
                    None
                }
            };

            let record = TransferRecord {
                id: id.clone(),
                from_did: from_did.clone(),
                to_did: body.to_did.clone(),
                amount: body.amount,
                timestamp: now_ms,
                status: "completed".to_string(),
                new_balance,
                event_id: event_id.clone(),
            };
            crate::state::record_transfer(&state.transfer_history, record).await;

            tracing::info!(
                from_did = %from_did,
                to_did = %body.to_did,
                amount = body.amount,
                new_balance = new_balance,
                transfer_id = %id,
                event_id = ?event_id,
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
                    "event_id": event_id,
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
    let records: Vec<&TransferRecord> = history.iter().rev().take(limit).collect();
    let count = records.len();
    let total = history.len();

    // When Lane 0 is enabled, annotate each record that has a provenance
    // event with its fast-path finality status (single substrate read lock
    // for the whole page).
    let substrate = state.substrate.read().await;
    let lane0_enabled = substrate.lane0_enabled();
    let transfers: Vec<Value> = records
        .into_iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or_else(|_| json!({}));
            if lane0_enabled {
                let is_final = r
                    .event_id
                    .as_deref()
                    .and_then(crate::api::events::decode_event_id)
                    .map(|id| substrate.lane0_is_final(&id))
                    .unwrap_or(false);
                v["lane0_final"] = json!(is_final);
            }
            v
        })
        .collect();
    drop(substrate);

    Json(json!({
        "transfers": transfers,
        "count": count,
        "total_in_history": total,
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
    fn test_transfer_payload_roundtrip() {
        let p = TransferEventPayload {
            from_did: "did:omnia:aaaa".to_string(),
            to_did: "did:omnia:bbbb".to_string(),
            amount: 250,
            transfer_id: "cafef00d".to_string(),
            timestamp: 1_752_000_000_000,
        };
        let bytes = encode_transfer_payload(&p);
        // Tagged so it's self-identifying and never a valid ShardPayload.
        assert!(bytes.starts_with(TRANSFER_PAYLOAD_TAG));
        let body = &bytes[TRANSFER_PAYLOAD_TAG.len()..];
        let decoded: TransferEventPayload = postcard::from_bytes(body).unwrap();
        assert_eq!(decoded.from_did, p.from_did);
        assert_eq!(decoded.to_did, p.to_did);
        assert_eq!(decoded.amount, p.amount);
        assert_eq!(decoded.transfer_id, p.transfer_id);
        assert_eq!(decoded.timestamp, p.timestamp);
    }

    #[test]
    fn test_transfer_payload_not_a_shard_payload() {
        // The router must skip transfer receipts (they aren't shard ops).
        let p = TransferEventPayload {
            from_did: "did:omnia:aaaa".to_string(),
            to_did: "did:omnia:bbbb".to_string(),
            amount: 1,
            transfer_id: "id".to_string(),
            timestamp: 0,
        };
        let bytes = encode_transfer_payload(&p);
        assert!(
            omnia_shards::ShardPayload::from_bytes(&bytes).is_err(),
            "transfer receipt must not deserialize as a ShardPayload"
        );
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
