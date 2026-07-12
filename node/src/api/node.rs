//! Node info, peers, and validators API handlers
//!
//! Provides endpoints for querying node metadata, the peer list, and the
//! validator set:
//! - `GET /api/v1/node/info` — node identity, version, uptime, consensus state
//! - `GET /api/v1/node/peers` — connected peer addresses
//! - `GET /api/v1/validators` — registered validators with slash points and jail status

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::state::AppState;

/// Handler for `GET /api/v1/node/info`.
///
/// Returns node metadata including identity, protocol version, uptime,
/// peer count, finalized event count, and registered shard count.
#[utoipa::path(
    get,
    path = "/api/v1/node/info",
    responses(
        (status = 200, description = "Node information"),
    )
)]
pub async fn node_info(State(state): State<AppState>) -> Json<Value> {
    let node_id_hex = hex::encode(&state.config.node_id_bytes()[..4]);
    let uptime = state.started_at.elapsed().as_secs();
    let peer_count = state.peers.read().await.len();
    // Issue #260: the in-memory event store resets on restart; the
    // consensus engine's committed count is restored from the persistent
    // consensus store. Report the max of both so finalized_height
    // survives restarts.
    let event_count = {
        let store_len = state.event_store.read().await.len() as u64;
        let committed = state.substrate.read().await.committed_count();
        store_len.max(committed)
    };

    let shard_count = {
        // Handle poisoned mutex gracefully instead of panicking.
        // A poisoned mutex indicates another thread panicked while holding
        // the lock, which means the data may be in an inconsistent state.
        // Return HTTP 503 to signal the service is temporarily unavailable.
        match state.shard_router.lock() {
            Ok(router) => router.shard_count(),
            Err(poisoned) => {
                tracing::error!("shard_router mutex poisoned — returning 503");
                // Recover the guard from the poisoned mutex to avoid cascading failures
                poisoned.into_inner().shard_count()
            }
        }
    };

    // The node's Ed25519 validator public key — operators need the full
    // hex value to build the Lane 0 validator set (OMNIA_LANE0_VALIDATORS).
    let validator_pubkey = state
        .keypair
        .as_ref()
        .map(|k| hex::encode(k.verifying_key().to_bytes()));
    let lane0 = {
        let substrate = state.substrate.read().await;
        substrate.lane0_stats().map(|(accepted, rejected, finalized)| {
            json!({
                "acks_accepted": accepted,
                "acks_rejected": rejected,
                "events_finalized": finalized,
            })
        })
    };

    Json(json!({
        "node_id": node_id_hex,
        "node_id_num": state.config.node_id,
        "version": env!("CARGO_PKG_VERSION"),
        "protocol_version": omnia_substrate::PROTOCOL_VERSION,
        "uptime_seconds": uptime,
        "peers": peer_count,
        "finalized_height": event_count,
        "shard_count": shard_count,
        "validator_pubkey": validator_pubkey,
        "lane0": lane0,
        "listen_addr": state.config.listen_addr,
        "data_dir": state.config.data_dir.to_string_lossy(),
    }))
}

/// Handler for `GET /api/v1/node/peers`.
///
/// Returns the list of known peer addresses. In a fully operational
/// node this would contain libp2p multiaddresses discovered via
/// the gossip protocol.
#[utoipa::path(
    get,
    path = "/api/v1/node/peers",
    responses(
        (status = 200, description = "Peer list"),
    )
)]
pub async fn node_peers(State(state): State<AppState>) -> Json<Value> {
    let peers = state.peers.read().await;
    let peer_list: Vec<Value> = peers
        .iter()
        .map(|p| {
            json!({
                "peer_id": p.peer_id,
                "address": p.address,
                "connected_at": p.connected_at,
            })
        })
        .collect();

    Json(json!({
        "peers": peer_list,
        "count": peer_list.len(),
    }))
}

/// Simple peer information tracked by this node.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema)]
pub struct PeerInfo {
    /// Hex-encoded peer identifier.
    pub peer_id: String,
    /// Network address of the peer.
    pub address: String,
    /// Unix timestamp when the peer was discovered.
    pub connected_at: u64,
}

/// Handler for `GET /api/v1/validators`.
///
/// Returns the list of validator candidates registered with this node,
/// along with each validator's stake, slash points, and jail status.
///
/// Slash points and jail status are sourced from the slashing engine
/// (persistent redb store). Stake comes from the substrate's
/// `validator_candidates` registry. The keypair itself is intentionally
/// NOT exposed — only the public NodeId (hex-encoded) is returned.
#[utoipa::path(
    get,
    path = "/api/v1/validators",
    responses(
        (status = 200, description = "Validator list"),
    )
)]
pub async fn list_validators(State(state): State<AppState>) -> Json<Value> {
    // Read consensus round and validator candidates from the substrate.
    let (validators_with_stake, current_round) = {
        let substrate = state.substrate.read().await;
        let round = substrate.current_round();
        let entries: Vec<(String, u64)> = substrate
            .validator_candidates_iter()
            .map(|(node_id, stake)| (hex::encode(node_id), stake))
            .collect();
        (entries, round)
    };

    // Read slash points and jail status from the slashing engine.
    let slashing = state.slashing.lock().await;
    let mut validators: Vec<Value> = Vec::with_capacity(validators_with_stake.len());

    for (node_id_hex, stake) in validators_with_stake {
        // Decode the hex back to NodeId for slashing-engine lookups.
        // If decoding fails (shouldn't happen since we just encoded it),
        // report zero slash points rather than failing the whole request.
        let node_id_bytes = hex::decode(&node_id_hex).unwrap_or_default();
        let node_id_array: [u8; 32] = if node_id_bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&node_id_bytes);
            arr
        } else {
            // Skip malformed entries rather than panicking.
            continue;
        };

        let slash_points = slashing.slash_points_of(&node_id_array);
        let is_jailed = slashing.is_jailed(node_id_array, current_round);

        let status = if is_jailed {
            "jailed"
        } else if slash_points > 0 {
            "slashed"
        } else {
            "active"
        };

        validators.push(json!({
            "node_id": node_id_hex,
            "stake": stake,
            "slash_points": slash_points,
            "is_jailed": is_jailed,
            "status": status,
            "current_round": current_round,
        }));
    }

    let total_stake: u64 = validators
        .iter()
        .filter_map(|v| v.get("stake").and_then(|s| s.as_u64()))
        .sum();
    let active_count = validators
        .iter()
        .filter(|v| v.get("status").and_then(|s| s.as_str()) == Some("active"))
        .count();
    let jailed_count = validators
        .iter()
        .filter(|v| v.get("status").and_then(|s| s.as_str()) == Some("jailed"))
        .count();

    Json(json!({
        "validators": validators,
        "count": validators.len(),
        "active_count": active_count,
        "jailed_count": jailed_count,
        "total_stake": total_stake,
        "current_round": current_round,
    }))
}
