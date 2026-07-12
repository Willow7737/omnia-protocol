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

/// Wire prefix for **wallet-signed** transfer receipts (spend
/// authorization Step 2). The v2 payload embeds the wallet's public key,
/// the consumed nonce, and the Ed25519 signature over the canonical
/// transfer message, so the spend authorization itself is on-chain —
/// anyone can re-verify that the key owner, not the node, authorized it.
const TRANSFER_PAYLOAD_TAG_V2: &[u8] = b"OMNIA_XFER_V2";

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

/// On-chain provenance record for a **wallet-signed** transfer (v2).
///
/// Extends the v1 receipt with the self-sovereign authorization proof:
/// the wallet's Ed25519 public key, the single-use nonce it consumed, and
/// its signature over [`crate::api::wallet_auth::transfer_message`]
/// `(nonce, from_did, to_did, amount)`. `from_did` is derived from
/// `wallet_pubkey`, so the receipt is self-verifying.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SignedTransferEventPayload {
    transfer: TransferEventPayload,
    wallet_pubkey: [u8; 32],
    nonce: String,
    /// 64-byte Ed25519 signature (Vec because serde lacks `[u8; 64]` impls).
    signature: Vec<u8>,
}

/// Encode a wallet-signed transfer receipt as `TAG_V2 ++ postcard(payload)`.
fn encode_signed_transfer_payload(p: &SignedTransferEventPayload) -> Vec<u8> {
    let mut bytes = TRANSFER_PAYLOAD_TAG_V2.to_vec();
    if let Ok(body) = postcard::to_allocvec(p) {
        bytes.extend(body);
    }
    bytes
}

/// Run `f` against the single-source economics state, returning its result.
///
/// The economics state is owned by the registered economics shard inside
/// the shared shard router (`AppState.shard_router`), which is the SAME
/// `Arc<Mutex<ShardRouter>>` the substrate's shard processor mutates on the
/// event/consensus path. Reading and writing through here therefore keeps
/// the API and the event path on one store — no divergent second copy
/// (the C4 audit finding).
///
/// The router guard is `std::sync` and `f` is synchronous, so the guard is
/// always dropped before the caller can `.await` (the compiler enforces
/// this — a held `std` guard would make the handler future `!Send`).
pub(crate) fn with_economics<R>(
    state: &AppState,
    f: impl FnOnce(&mut omnia_economics::EconomicsState) -> R,
) -> Result<R, (StatusCode, Json<Value>)> {
    let mut router = state
        .shard_router
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let econ = router.economics_mut().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "economics shard not registered"})),
        )
    })?;
    Ok(f(econ))
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
    /// Optional wallet-signed spend authorization (Step 2, self-sovereign
    /// transfers). When present, the node verifies the wallet's own
    /// Ed25519 signature over the transfer before spending — the node
    /// merely attests receipt; the *key owner* authorizes the spend.
    /// When absent, the transfer is authorized by the JWT alone
    /// (node-attested, the v1 behavior — kept for deployed clients).
    #[serde(default)]
    pub authorization: Option<TransferAuthorization>,
}

/// Wallet-signed spend authorization attached to a [`TransferRequest`].
///
/// Flow: the wallet requests a nonce from `POST /api/v1/auth/challenge`
/// (with its public key), signs the canonical transfer message
/// (`omnia-transfer-v1\n<nonce>\n<from_did>\n<to_did>\n<amount>`, where
/// `from_did` is derived from the public key), and attaches all three
/// fields here. The nonce is single-use — a captured authorization
/// cannot be replayed.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TransferAuthorization {
    /// Hex-encoded 32-byte Ed25519 public key of the spending wallet.
    pub public_key: String,
    /// The single-use nonce obtained from `/auth/challenge`.
    pub nonce: String,
    /// Hex-encoded 64-byte Ed25519 signature over the canonical
    /// transfer message.
    pub signature: String,
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
    let (balance, quota, is_registered, current_epoch) = with_economics(&state, |econ| {
        (
            econ.balance_of(&did),
            econ.quota.quota_of(&did),
            econ.quota.is_registered(&did),
            econ.current_epoch(),
        )
    })?;

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
        "current_epoch": current_epoch,
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

    // The canonical transfer message is newline-delimited; a DID
    // containing control characters could forge field boundaries. Reject
    // them outright (also plain hygiene for the unsigned path).
    if body.to_did.chars().any(char::is_control) || from_did.chars().any(char::is_control) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "DIDs must not contain control characters"})),
        ));
    }

    // Step 2 (self-sovereign spend): when a wallet-signed authorization is
    // attached, verify it BEFORE any state change. The signature — not
    // the JWT — is what authorizes the spend; the JWT still authenticates
    // the caller and the two identities must agree.
    let wallet_authorized = match &body.authorization {
        Some(auth) => {
            use crate::api::wallet_auth::{self, decode_hex_fixed};
            use omnia_substrate::crypto::{Signature, VerifyingKey};

            let pubkey_bytes = decode_hex_fixed::<32>(&auth.public_key).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("invalid authorization.public_key: {e}")})),
                )
            })?;
            let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("invalid authorization.public_key: {e}")})),
                )
            })?;
            let sig_bytes = decode_hex_fixed::<64>(&auth.signature).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("invalid authorization.signature: {e}")})),
                )
            })?;

            // The signing key's derived DID is the spender. It must match
            // the JWT identity — otherwise a stolen JWT could be paired
            // with an attacker's own key (or vice versa).
            let signer_did = wallet_auth::did_from_public_key(&pubkey_bytes);
            if signer_did != from_did {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "authorization key does not belong to the authenticated caller",
                        "authenticated_did": from_did,
                        "signer_did": signer_did,
                    })),
                ));
            }

            // Consume the single-use nonce (bound to this key), then
            // verify the signature over the canonical transfer message.
            let public_key_hex = hex::encode(pubkey_bytes);
            wallet_auth::consume_challenge(&state, &auth.nonce, &public_key_hex).await?;
            let message = wallet_auth::transfer_message(&auth.nonce, &from_did, &body.to_did, body.amount);
            verifying_key
                .verify_strict(message.as_bytes(), &Signature::from_bytes(&sig_bytes))
                .map_err(|_| {
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"error": "transfer signature verification failed"})),
                    )
                })?;

            Some((pubkey_bytes, auth.nonce.clone(), sig_bytes.to_vec()))
        }
        None => None,
    };
    let provenance = if wallet_authorized.is_some() {
        "wallet_signed"
    } else {
        "node_attested"
    };

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

    // Registration checks + the authoritative spend happen under the
    // shared economics lock (single source of truth). The guard is dropped
    // when the closure returns, before any `.await` below.
    let new_balance = with_economics(&state, |econ| {
        if !econ.quota.is_registered(&from_did) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("Sender DID not registered: {from_did}") })),
            ));
        }
        if !econ.quota.is_registered(&body.to_did) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("Recipient DID not registered: {}", body.to_did) })),
            ));
        }
        // Perform the spend (UBC is soulbound — no actual transfer).
        econ.quota.spend(&from_did, body.amount).map_err(|e| {
            tracing::warn!(from_did = %from_did, amount = body.amount, error = %e, "UBC spend operation failed");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Transfer failed: {e}"),
                    "from_did": from_did,
                    "amount": body.amount,
                })),
            )
        })?;
        Ok(econ.balance_of(&from_did).unwrap_or(0))
    })??;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Derive a stable transfer ID: BLAKE3 of (from_did, to_did, amount, timestamp).
    // Using BLAKE3 (rather than a UUID) keeps IDs deterministic and
    // auditable — anyone with the same inputs can recompute the ID.
    let id_input = format!("{from_did}|{}|{}|{now_ms}", body.to_did, body.amount);
    let id = blake3::hash(id_input.as_bytes()).to_hex().to_string();

    // Record the transfer on-chain as a signed causal-graph event:
    // provenance + Lane 0 fast-path finality + gossip propagation. The
    // authoritative balance change already happened above; if the
    // provenance event fails to submit, the transfer still succeeded — we
    // just record event_id: None and log it.
    let transfer_payload = TransferEventPayload {
        from_did: from_did.clone(),
        to_did: body.to_did.clone(),
        amount: body.amount,
        transfer_id: id.clone(),
        timestamp: now_ms,
    };
    // Wallet-signed transfers carry the authorization proof on-chain (v2
    // receipt); node-attested transfers keep the v1 receipt.
    let payload = match &wallet_authorized {
        Some((wallet_pubkey, nonce, signature)) => encode_signed_transfer_payload(&SignedTransferEventPayload {
            transfer: transfer_payload,
            wallet_pubkey: *wallet_pubkey,
            nonce: nonce.clone(),
            signature: signature.clone(),
        }),
        None => encode_transfer_payload(&transfer_payload),
    };
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
        provenance: provenance.to_string(),
    };
    crate::state::record_transfer(&state.transfer_history, record).await;

    tracing::info!(
        from_did = %from_did,
        to_did = %body.to_did,
        amount = body.amount,
        new_balance = new_balance,
        transfer_id = %id,
        event_id = ?event_id,
        provenance = provenance,
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
            "provenance": provenance,
            "note": "UBC is soulbound — tokens are spent (consumed), not transferred to the recipient",
        })),
    ))
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
    fn test_transfer_request_without_authorization_deserializes() {
        // Deployed v1 clients send no authorization block — must stay valid.
        let json = r#"{"from_did":"did:omnia:abc","to_did":"did:omnia:def","amount":100}"#;
        let req: TransferRequest = serde_json::from_str(json).unwrap();
        assert!(req.authorization.is_none());
    }

    #[test]
    fn test_transfer_request_with_authorization_deserializes() {
        let json = r#"{
            "from_did":"did:omnia:abc","to_did":"did:omnia:def","amount":100,
            "authorization":{"public_key":"aa","nonce":"bb","signature":"cc"}
        }"#;
        let req: TransferRequest = serde_json::from_str(json).unwrap();
        let auth = req.authorization.unwrap();
        assert_eq!(auth.public_key, "aa");
        assert_eq!(auth.nonce, "bb");
        assert_eq!(auth.signature, "cc");
    }

    #[test]
    fn test_signed_transfer_payload_roundtrip_and_not_shard_payload() {
        let p = SignedTransferEventPayload {
            transfer: TransferEventPayload {
                from_did: "did:omnia:aaaa".to_string(),
                to_did: "did:omnia:bbbb".to_string(),
                amount: 250,
                transfer_id: "cafef00d".to_string(),
                timestamp: 1_752_000_000_000,
            },
            wallet_pubkey: [7u8; 32],
            nonce: "deadbeef".to_string(),
            signature: vec![9u8; 64],
        };
        let bytes = encode_signed_transfer_payload(&p);
        assert!(bytes.starts_with(TRANSFER_PAYLOAD_TAG_V2));
        let body = &bytes[TRANSFER_PAYLOAD_TAG_V2.len()..];
        let decoded: SignedTransferEventPayload = postcard::from_bytes(body).unwrap();
        assert_eq!(decoded.transfer.from_did, p.transfer.from_did);
        assert_eq!(decoded.wallet_pubkey, p.wallet_pubkey);
        assert_eq!(decoded.nonce, p.nonce);
        assert_eq!(decoded.signature, p.signature);
        // The router must skip v2 receipts too (not a shard op).
        assert!(omnia_shards::ShardPayload::from_bytes(&bytes).is_err());
    }

    #[test]
    fn test_v2_signature_verifies_over_canonical_message() {
        use omnia_substrate::crypto::{Signer, SigningKey};
        use rand::RngCore;

        // The exact flow a wallet performs: derive DID, build the
        // canonical message, sign it — and the node-side verification
        // accepts it while rejecting any field mutation.
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        let sk = SigningKey::from_bytes(&seed);
        let vk = sk.verifying_key();
        let from_did = crate::api::wallet_auth::did_from_public_key(&vk.to_bytes());

        let nonce = "00ff00ff";
        let to_did = "did:omnia:recipient";
        let amount = 42u64;
        let message = crate::api::wallet_auth::transfer_message(nonce, &from_did, to_did, amount);
        let sig = sk.sign(message.as_bytes());

        assert!(vk.verify_strict(message.as_bytes(), &sig).is_ok());
        // Any mutated field must fail verification.
        let tampered_amount = crate::api::wallet_auth::transfer_message(nonce, &from_did, to_did, 43);
        assert!(vk.verify_strict(tampered_amount.as_bytes(), &sig).is_err());
        let tampered_recipient = crate::api::wallet_auth::transfer_message(nonce, &from_did, "did:omnia:evil", amount);
        assert!(vk.verify_strict(tampered_recipient.as_bytes(), &sig).is_err());
        let tampered_nonce = crate::api::wallet_auth::transfer_message("11ee11ee", &from_did, to_did, amount);
        assert!(vk.verify_strict(tampered_nonce.as_bytes(), &sig).is_err());
    }

    #[test]
    fn test_transfer_message_field_boundaries_unambiguous() {
        // Two different (to_did, amount) splits must never produce the
        // same message — newline delimiting guarantees it as long as
        // fields contain no newlines (enforced by the handler).
        let a = crate::api::wallet_auth::transfer_message("n", "did:omnia:x", "did:omnia:y", 12);
        let b = crate::api::wallet_auth::transfer_message("n", "did:omnia:x", "did:omnia:y\n1", 2);
        assert_ne!(a, b);
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
