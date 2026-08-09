//! Admin-only endpoints for manual operational actions.
//!
//! These endpoints allow operators to manually trigger settlement
//! submissions and other maintenance tasks that are not yet automated
//! (see ADR-025 gap: no specified trigger mechanism for submissions).
//!
//! # Security
//!
//! All admin endpoints require **JWT authentication** (enforced by the
//! auth middleware layer in `mod.rs`). The settlement endpoint reads the
//! fleet's actual Lane 0 leading root from consensus state rather than
//! trusting a caller-supplied value, eliminating "sent the wrong root by
//! accident" as a failure category entirely.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Request body for submitting a state root to the settlement layer.
///
/// The `root` field is **ignored in normal operation** — the handler
/// reads the fleet's actual Lane 0 leading root from consensus state.
/// Set `OMNIA_SETTLEMENT_ALLOW_CUSTOM_ROOT=true` to honour a
/// caller-supplied value (e.g. for re-anchoring a historical root during
/// debugging).
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct SubmitRootRequest {
    /// Optional hex-encoded 32-byte override root (with or without `0x`
    /// prefix). Ignored unless `OMNIA_SETTLEMENT_ALLOW_CUSTOM_ROOT=true`.
    #[serde(default)]
    pub root: Option<String>,
}

/// Response body for a successful root submission.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SubmitRootResponse {
    /// The 32-byte state root that was submitted (hex with `0x` prefix).
    pub root: String,
    /// Transaction hash of the on-chain anchor transaction (hex).
    pub tx_hash: String,
    /// Block explorer URL for the anchor transaction.
    ///
    /// Populated when the settlement adapter targets a known network
    /// (e.g. Bitcoin testnet4). `None` for mock or unrecognised networks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explorer_url: Option<String>,
}

/// Submit the fleet's current Lane 0 leading root to the settlement layer.
///
/// This is the operator-triggered path from the ADR-025 gap analysis.
/// The handler **reads the actual leading root from consensus state** rather
/// than trusting the request body, which eliminates an entire class of
/// errors: accidental wrong-root submissions, replay of stale roots, and
/// unauthenticated anchoring of arbitrary data.
///
/// A caller-supplied override is accepted only when the debug env var
/// `OMNIA_SETTLEMENT_ALLOW_CUSTOM_ROOT=true` is set.
///
/// # Authentication
///
/// This endpoint is mounted in the authenticated route group — a valid
/// JWT is required to reach this handler.
///
/// # Errors
///
/// - `404` — Lane 0 not enabled or no root available (nothing to anchor)
/// - `503` — settlement adapter is not live (mock/stub)
/// - `500` — settlement submission failed on L1
#[utoipa::path(
    post,
    path = "/admin/settlement/submit-root",
    request_body = Option<SubmitRootRequest>,
    responses(
        (status = 200, description = "Root submitted successfully", body = SubmitRootResponse),
        (status = 404, description = "Lane 0 not enabled or no leading root"),
        (status = 503, description = "Settlement adapter is not live"),
        (status = 500, description = "L1 submission failed"),
    ),
    security("bearer_auth" = []),
    tag = "Admin"
)]
pub async fn submit_root(
    State(state): State<AppState>,
    req: Option<Json<SubmitRootRequest>>,
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

    // --- Determine the root to submit ---
    let allow_custom = std::env::var("OMNIA_SETTLEMENT_ALLOW_CUSTOM_ROOT")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);

    let root = if allow_custom {
        // Debug mode: accept caller-supplied root.
        let req_body = req
            .ok_or_else(|| (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "request body with \"root\" field required when OMNIA_SETTLEMENT_ALLOW_CUSTOM_ROOT is set" })),
            ))?
            .0;
        let root_str = req_body.root.as_deref().ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "\"root\" field is required in the request body" })),
            )
        })?;
        let hex_str = root_str.strip_prefix("0x").unwrap_or(root_str);
        let root_bytes = hex::decode(hex_str).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid hex in root: {e}") })),
            )
        })?;
        if root_bytes.len() != 32 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("root must be 32 bytes, got {}", root_bytes.len()) })),
            ));
        }
        let mut r = [0u8; 32];
        r.copy_from_slice(&root_bytes);
        r
    } else {
        // Normal mode: pull the fleet's actual Lane 0 leading root from
        // consensus state. This is the value the validator majority agreed
        // on — not an arbitrary caller-supplied value.
        state
            .substrate
            .read()
            .await
            .lane0_leading_root()
            .ok_or_else(|| (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "no Lane 0 leading root available — Lane 0 may be disabled or no events have been finalized yet"
                })),
            ))?
    };

    let root_hex = format!("0x{}", hex::encode(root));

    // --- Submit to L1 ---
    match state.settlement.submit_root(root).await {
        Ok(tx_hash) => {
            let explorer_url = explorer_url_for_tx(&tx_hash);
            Ok((
                StatusCode::OK,
                Json(SubmitRootResponse {
                    root: root_hex,
                    tx_hash: tx_hash.to_string(),
                    explorer_url,
                }),
            ))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("settlement submission failed: {e}")
            })),
        )),
    }
}

/// Return a block explorer URL for the given transaction hash,
/// based on the configured settlement network.
///
/// Currently supports:
/// - Bitcoin testnet4 → `https://mempool.space/testnet4/tx/<txid>`
/// - Bitcoin mainnet → `https://mempool.space/tx/<txid>`
///
/// Returns `None` for mock adapters or unknown networks.
fn explorer_url_for_tx(tx: &omnia_adapters::TxHash) -> Option<String> {
    let rpc_url = std::env::var("OMNIA_BITCOIN_RPC_URL").ok()?;
    let txid = tx.to_string();

    if rpc_url.contains("testnet4") || rpc_url.contains("18444") {
        return Some(format!("https://mempool.space/testnet4/tx/{txid}"));
    }
    if rpc_url.contains("testnet") || rpc_url.contains("18332") {
        return Some(format!("https://mempool.space/testnet/tx/{txid}"));
    }
    if rpc_url.contains("mainnet") || rpc_url.contains("8332") {
        return Some(format!("https://mempool.space/tx/{txid}"));
    }
    None
}
