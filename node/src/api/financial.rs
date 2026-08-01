//! Financial shard API handlers — the transferable-asset ledger.
//!
//! - `GET  /api/v1/financial/balance/:pubkey` — balance and next transfer nonce
//! - `POST /api/v1/financial/transfer` — submit a wallet-signed transfer
//!
//! # Why this is separate from `/economics`
//!
//! Omnia runs two economies and they are not interchangeable:
//!
//! | | `/economics` (UBC) | `/financial` (this module) |
//! |---|---|---|
//! | Nature | Soulbound compute right | Transferable asset |
//! | "Send" | Spends/burns; recipient is **not** credited | Debits sender, **credits recipient** |
//! | Supply | Reset to quota each epoch | Conserved by transfers |
//! | Addressed by | `did:omnia:…` | Ed25519 public key |
//!
//! UBC deliberately cannot move between identities — that is what stops
//! participation rights from being bought up and concentrated. This module
//! is the other half: value that genuinely moves from one person to
//! another. Clients that want to pay someone want this endpoint; clients
//! metering compute want `/economics`.
//!
//! # Why accounts are public keys, not DIDs
//!
//! A `did:omnia:` is `sha256(pubkey)` truncated to 32 hex chars — a
//! one-way hash. The financial shard verifies a sender's Ed25519
//! signature, so it needs the actual verifying key; a DID cannot be
//! converted back into one. Addressing accounts by public key also means
//! a recipient needs no prior contact with any node to be paid, and
//! anyone holding the key can still derive the DID from it.
//!
//! # Trust model
//!
//! The node never holds spending authority. A transfer is authorized by
//! an Ed25519 signature the wallet produces over
//! [`omnia_shards::financial::ops::signed_transfer_message`]; the node
//! relays it and signs the carrying event, and the shard re-verifies the
//! signature on every node that applies it. A malicious node can refuse
//! to relay a transfer, but it cannot forge or alter one.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use omnia_shards::financial::ops::{signed_transfer_message, AccountId, FinancialOp};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::api::auth::CallerIdentity;
use crate::api::events::build_sign_submit_event;
use crate::api::wallet_auth::{decode_hex_fixed, did_from_public_key};
use crate::state::AppState;

/// Domain-tagged wire prefix for financial-transfer event payloads.
///
/// Distinct from the UBC receipt tags in [`crate::api::economics`] so a
/// decoder can never confuse a value-moving transfer with a UBC spend.
const FINANCIAL_TRANSFER_TAG: &[u8] = b"OMNIA_FIN_XFER_V1";

/// On-chain record of a financial transfer, embedded (tagged +
/// postcard-encoded) in the payload of a causal-graph event.
///
/// Carries the full authorization, so any observer can independently
/// re-verify that the holder of `from` — not the relaying node —
/// authorized this movement of value.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FinancialTransferPayload {
    from: [u8; 32],
    to: [u8; 32],
    amount: u64,
    nonce: u64,
    /// 64-byte Ed25519 signature (`Vec` because serde lacks `[u8; 64]` impls).
    signature: Vec<u8>,
}

/// Encode a transfer record as `TAG ++ postcard(payload)`.
fn encode_transfer_payload(p: &FinancialTransferPayload) -> Vec<u8> {
    let mut bytes = FINANCIAL_TRANSFER_TAG.to_vec();
    if let Ok(body) = postcard::to_allocvec(p) {
        bytes.extend(body);
    }
    bytes
}

/// Run `f` against the single-source financial state.
///
/// Mirrors [`crate::api::economics::with_economics`]: the state lives in
/// the financial shard inside the shared `AppState.shard_router`, which is
/// the same `Arc<Mutex<ShardRouter>>` the substrate's event path mutates.
/// Reading and writing through here keeps the API and the event path on
/// one store rather than letting a second copy drift.
///
/// The router guard is `std::sync` and `f` is synchronous, so the guard is
/// always dropped before the caller can `.await` — the compiler enforces
/// this, since a held `std` guard would make the handler future `!Send`.
fn with_financial<R>(
    state: &AppState,
    f: impl FnOnce(&mut omnia_shards::FinancialState) -> R,
) -> Result<R, (StatusCode, Json<Value>)> {
    let mut router = state
        .shard_router
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let financial = router.financial_mut().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "financial shard not registered"})),
        )
    })?;
    Ok(f(financial))
}

/// Parse a hex-encoded Ed25519 public key into an [`AccountId`].
fn parse_account(field: &str, value: &str) -> Result<AccountId, (StatusCode, Json<Value>)> {
    decode_hex_fixed::<32>(value).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("invalid {field}: expected 64 hex characters (an Ed25519 public key): {e}"),
            })),
        )
    })
}

/// Handler for `GET /api/v1/financial/balance/:pubkey`.
///
/// Returns the account's transferable balance and the nonce its next
/// transfer must use. Clients need `next_nonce` to build a signature that
/// will be accepted — the shard requires strictly increasing nonces.
#[utoipa::path(
    get,
    path = "/api/v1/financial/balance/{pubkey}",
    params(("pubkey" = String, Path, description = "Hex-encoded Ed25519 public key")),
    responses(
        (status = 200, description = "Account balance"),
        (status = 400, description = "Malformed public key"),
    )
)]
pub async fn get_balance(
    State(state): State<AppState>,
    Path(pubkey): Path<String>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let account = parse_account("pubkey", &pubkey)?;

    let (balance, last_nonce, total_supply) = with_financial(&state, |fin| {
        (
            fin.balance_of(&account),
            fin.signed_transfer_nonces.get(&account).copied(),
            fin.total_supply,
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "public_key": hex::encode(account),
            "did": did_from_public_key(&account),
            "balance": balance,
            // The shard requires nonce > last accepted, so the next usable
            // value is last + 1 (or 1 for an account that has never sent).
            "next_nonce": last_nonce.map(|n| n.saturating_add(1)).unwrap_or(1),
            "last_nonce": last_nonce,
            "total_supply": total_supply,
            "asset": "OMNIA",
            "transferable": true,
        })),
    ))
}

/// Request body for `POST /api/v1/financial/transfer`.
///
/// Every field is covered by `signature`; the node cannot alter any of
/// them without invalidating it.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct FinancialTransferRequest {
    /// Hex-encoded Ed25519 public key of the sender (also the verifying key).
    pub from: String,
    /// Hex-encoded Ed25519 public key of the recipient.
    pub to: String,
    /// Amount to transfer. Must be greater than zero.
    pub amount: u64,
    /// Strictly increasing per-sender nonce. Fetch `next_nonce` from
    /// `GET /api/v1/financial/balance/{pubkey}`.
    pub nonce: u64,
    /// Hex-encoded 64-byte Ed25519 signature over the canonical message
    /// `"omnia-financial-transfer:v1" || from || to || amount_le || nonce_le`.
    pub signature: String,
}

/// Handler for `POST /api/v1/financial/transfer`.
///
/// Moves transferable balance from `from` to `to`. Unlike the UBC
/// endpoint, this credits the recipient — total supply is conserved.
///
/// The JWT authenticates *who is asking the node to relay*; the Ed25519
/// signature authorizes *moving the funds*. Both must point at the same
/// identity, so a stolen JWT cannot be paired with an attacker's own key
/// and a stolen key cannot be relayed under someone else's session.
#[utoipa::path(
    post,
    path = "/api/v1/financial/transfer",
    request_body = FinancialTransferRequest,
    responses(
        (status = 200, description = "Transfer applied"),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Signature verification failed"),
        (status = 403, description = "Signing key does not belong to the authenticated caller"),
    )
)]
pub async fn transfer(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
    Json(body): Json<FinancialTransferRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let from = parse_account("from", &body.from)?;
    let to = parse_account("to", &body.to)?;
    let signature = decode_hex_fixed::<64>(&body.signature).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("invalid signature: expected 128 hex characters: {e}")})),
        )
    })?;

    if body.amount == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Transfer amount must be greater than zero"})),
        ));
    }
    if from == to {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot transfer to the sending account"})),
        ));
    }

    // The signing key must belong to the authenticated caller. Without
    // this, a valid JWT for identity A could relay a transfer signed by
    // key B — still unforgeable (the shard checks the signature), but it
    // would let one account launder its transfers through another's
    // session and muddy any per-caller accounting or rate limiting.
    let signer_did = did_from_public_key(&from);
    if signer_did != caller.caller_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "signing key does not belong to the authenticated caller",
                "authenticated_did": caller.caller_id,
                "signer_did": signer_did,
            })),
        ));
    }

    // Verify the signature here as well as in the shard. The shard's check
    // is the authority — it is what every other node re-runs — but doing
    // it up front means a bad signature is rejected with a clear 401
    // instead of being written into an event that then fails to apply.
    {
        use omnia_substrate::crypto::{Signature, VerifyingKey};
        let verifying_key = VerifyingKey::from_bytes(&from).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("sender is not a valid Ed25519 public key: {e}")})),
            )
        })?;
        let message = signed_transfer_message(&from, &to, body.amount, body.nonce);
        verifying_key
            .verify_strict(&message, &Signature::from_bytes(&signature))
            .map_err(|_| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "transfer signature verification failed"})),
                )
            })?;
    }

    let op = FinancialOp::SignedTransfer {
        from,
        to,
        amount: body.amount,
        nonce: body.nonce,
        signature: signature.to_vec(),
    };

    // Pre-flight against current state so an unaffordable or replayed
    // transfer fails before it is written to the graph. This is advisory:
    // apply re-checks everything under the same lock below.
    with_financial(&state, |fin| omnia_shards::FinancialValidator::validate(fin, &op))?.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Transfer rejected: {e}")})),
        )
    })?;

    // Record the transfer on the causal graph first. The payload carries
    // the authorization, so the event is a self-contained, independently
    // verifiable receipt that gossips to peers.
    let payload = encode_transfer_payload(&FinancialTransferPayload {
        from,
        to,
        amount: body.amount,
        nonce: body.nonce,
        signature: signature.to_vec(),
    });
    let submitted = build_sign_submit_event(&state, payload).await?;

    // Authoritative application under the shared lock, using the causal
    // context of the event just submitted. The sender comes from the
    // signature, never from that event's creator.
    let (sender_balance, recipient_balance) = with_financial(&state, |fin| {
        fin.apply_signed_transfer(&from, &to, body.amount, body.nonce, &signature, &submitted.vector_clock)
            .map(|()| (fin.balance_of(&from), fin.balance_of(&to)))
    })?
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Transfer failed: {e}"),
                "event_id": submitted.event_id_hex,
            })),
        )
    })?;

    tracing::info!(
        from = %hex::encode(from),
        to = %hex::encode(to),
        amount = body.amount,
        nonce = body.nonce,
        event_id = %submitted.event_id_hex,
        "Financial transfer applied"
    );

    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "applied",
            "from": hex::encode(from),
            "to": hex::encode(to),
            "amount": body.amount,
            "nonce": body.nonce,
            "sender_balance": sender_balance,
            "recipient_balance": recipient_balance,
            "event_id": submitted.event_id_hex,
            "authorization": "wallet_signed",
        })),
    ))
}
