//! Node info and peers API handlers
//!
//! Provides endpoints for querying node metadata and the peer list:
//! - `GET /api/v1/node/info` — node identity, version, uptime, consensus state
//! - `GET /api/v1/node/peers` — connected peer addresses

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;

/// Handler for `GET /api/v1/node/info`.
///
/// Returns node metadata including identity, protocol version, uptime,
/// peer count, finalized event count, and registered shard count.
pub async fn node_info(State(state): State<AppState>) -> Json<Value> {
    let node_id_hex = hex::encode(&state.config.node_id_bytes()[..4]);
    let uptime = state.started_at.elapsed().as_secs();
    let peer_count = state.peers.read().await.len();
    let event_count = state.event_store.read().await.len();

    let shard_count = {
        let router = state.shard_router.lock().await;
        router.shard_count()
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
        "listen_addr": state.config.listen_addr,
        "data_dir": state.config.data_dir.to_string_lossy(),
    }))
}

/// Handler for `GET /api/v1/node/peers`.
///
/// Returns the list of known peer addresses. In a fully operational
/// node this would contain libp2p multiaddresses discovered via
/// the gossip protocol.
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
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Hex-encoded peer identifier.
    pub peer_id: String,
    /// Network address of the peer.
    pub address: String,
    /// Unix timestamp when the peer was discovered.
    pub connected_at: u64,
}
