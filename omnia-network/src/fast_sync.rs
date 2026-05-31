//! Fast-sync protocol for late-joining nodes.
//!
//! Instead of replaying all events from genesis, new nodes can:
//! 1. Download a recent state snapshot from peers
//! 2. Verify snapshot integrity via BLAKE3
//! 3. Replay only the delta events since the snapshot
//!
//! # P2P Automation
//!
//! The [`SyncNetwork`] trait abstracts the networking layer so that
//! [`FastSyncManager`] can query peers and download snapshots without
//! depending on a specific networking implementation (libp2p, mock, etc.).
//! The full sync loop ([`FastSyncManager::sync_to_latest`]) performs:
//!
//! 1. Query connected peers for their latest checkpoint
//! 2. Select target checkpoint via supermajority agreement
//! 3. Download the snapshot from the target peer
//! 4. Verify snapshot integrity (BLAKE3 domain-separated hash)
//! 5. Deserialize and apply the snapshot
//! 6. Download and replay delta events since the snapshot round

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::blake3_domain::blake3_hash_domain;
use omnia_primitives::NodeId;

/// Maximum number of rounds to request in a single GetEvents sync.
const MAX_SYNC_ROUNDS: u64 = 10_000;

/// Maximum allowed snapshot size (64 MiB).
const MAX_SNAPSHOT_SIZE: usize = 64 * 1024 * 1024;

/// Maximum number of individual events in a SyncResponse::Events.
/// Prevents memory exhaustion from a malicious peer sending an
/// extremely large event list.
const MAX_SYNC_EVENTS_COUNT: usize = 100_000;

/// Maximum total bytes across all events in a SyncResponse::Events.
/// Prevents memory exhaustion from a malicious peer sending events
/// with very large individual payloads.
const MAX_SYNC_EVENTS_TOTAL_BYTES: usize = 512 * 1024 * 1024; // 512 MiB

/// Errors that can occur during fast sync.
#[derive(Error, Debug)]
pub enum SyncError {
    /// No peers available for snapshot download.
    #[error("no peers available for snapshot download")]
    NoPeersAvailable,
    /// Snapshot integrity verification failed.
    #[error("snapshot integrity check failed: expected {expected}, got {actual}")]
    IntegrityCheckFailed {
        /// Expected BLAKE3 hash.
        expected: String,
        /// Actual BLAKE3 hash computed from the snapshot data.
        actual: String,
    },
    /// Insufficient peer agreement on the latest checkpoint.
    #[error("insufficient peer agreement: {agreeing}/{total}")]
    InsufficientAgreement {
        /// Number of agreeing peers.
        agreeing: usize,
        /// Total number of peers queried.
        total: usize,
    },
    /// Network error during sync.
    #[error("network error: {0}")]
    Network(String),
    /// Consensus error during delta replay.
    #[error("consensus error during replay: {0}")]
    Consensus(String),
}

/// Network interface for fast-sync P2P operations.
///
/// This trait abstracts over the network layer, allowing the fast-sync
/// manager to query peers and download snapshots without depending on
/// a specific networking implementation (libp2p, mock, etc.).
pub trait SyncNetwork: Send + Sync {
    /// Get the list of currently connected peer IDs.
    fn connected_peers(&self) -> Vec<NodeId>;

    /// Send a sync request to a specific peer and get a response.
    fn send_request(&self, peer_id: NodeId, request: SyncRequest) -> Result<SyncResponse, SyncError>;
}

/// A serializable snapshot of consensus state at a given round.
///
/// Contains all the data needed to reconstruct the consensus engine's
/// state at the checkpoint's round. Snapshots are serialized with
/// postcard for compact storage and fast deserialization.
///
/// Note: This type is distinct from the full persistent snapshot format
/// in the substrate. [`SyncSnapshot`] is the compact P2P wire format
/// used during fast-sync transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSnapshot {
    /// The round this snapshot represents.
    pub round: u64,
    /// The state root hash at this round.
    pub state_root: [u8; 32],
    /// Serialized causal graph state.
    pub causal_graph_data: Vec<u8>,
    /// Serialized consensus state.
    pub consensus_data: Vec<u8>,
    /// Number of events in the snapshot.
    pub event_count: u64,
}

/// A checkpoint representing a known-good state at a specific round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCheckpoint {
    /// The consensus round this checkpoint represents.
    pub round: u64,
    /// Merkle root of the state at this round.
    pub state_root: [u8; 32],
    /// BLAKE3 hash of the serialized snapshot data.
    pub snapshot_hash: [u8; 32],
    /// Number of events committed up to this checkpoint.
    pub event_count: u64,
    /// Timestamp (epoch millis) when the checkpoint was created.
    pub timestamp: u64,
    /// The peer that provided this checkpoint.
    pub peer_id: Option<NodeId>,
}

impl SyncCheckpoint {
    /// Verify that a snapshot's hash matches this checkpoint.
    pub fn verify_snapshot(&self, snapshot_data: &[u8]) -> Result<(), SyncError> {
        let computed = blake3_hash_domain(b"OMNIA-FAST-SYNC-V1", snapshot_data);
        if computed != self.snapshot_hash {
            return Err(SyncError::IntegrityCheckFailed {
                expected: hex::encode(self.snapshot_hash),
                actual: hex::encode(computed),
            });
        }
        Ok(())
    }
}

/// Result of a fast sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// The round the node synced to.
    pub synced_to_round: u64,
    /// Number of delta events replayed after snapshot application.
    pub events_replayed: u64,
    /// Hash of the applied snapshot.
    pub snapshot_hash: [u8; 32],
}

/// Sync protocol request types for the request-response protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncRequest {
    /// Request the peer's latest checkpoint.
    GetCheckpoint,
    /// Request a snapshot by its hash.
    GetSnapshot {
        /// BLAKE3 hash of the desired snapshot.
        checkpoint_hash: [u8; 32],
    },
    /// Request events between two rounds (inclusive).
    GetEvents {
        /// Start round (inclusive).
        from_round: u64,
        /// End round (inclusive).
        to_round: u64,
    },
}

/// Sync protocol response types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncResponse {
    /// Checkpoint response.
    Checkpoint(Option<SyncCheckpoint>),
    /// Snapshot data response.
    Snapshot(Option<Vec<u8>>),
    /// Events response.
    Events(Vec<Vec<u8>>),
}

/// Select the target checkpoint from multiple peer responses.
///
/// Uses supermajority agreement: the checkpoint with the highest round
/// that is agreed upon by at least 2/3 of the responding peers.
pub fn select_target_checkpoint(
    checkpoints: &[SyncCheckpoint],
    total_peers: usize,
) -> Result<SyncCheckpoint, SyncError> {
    if checkpoints.is_empty() {
        return Err(SyncError::NoPeersAvailable);
    }

    // Group by (round, state_root) to find agreement
    let mut agreement_map: HashMap<(u64, [u8; 32]), Vec<&SyncCheckpoint>> = HashMap::new();
    for cp in checkpoints {
        let key = (cp.round, cp.state_root);
        agreement_map.entry(key).or_default().push(cp);
    }

    // Find the group with supermajority agreement and highest round
    let supermajority = (2 * total_peers).div_ceil(3);
    let mut best: Option<SyncCheckpoint> = None;

    for ((round, _), cps) in &agreement_map {
        if cps.len() >= supermajority && (*round > best.as_ref().map(|b| b.round).unwrap_or(0)) {
            best = Some((*cps[0]).clone());
        }
    }

    best.ok_or(SyncError::InsufficientAgreement {
        agreeing: agreement_map.values().map(|v| v.len()).max().unwrap_or(0),
        total: total_peers,
    })
}

/// Fast-sync manager for coordinating snapshot download and delta replay.
///
/// Coordinates the full P2P fast-sync loop: query peers for checkpoints,
/// select a target via supermajority agreement, download and verify the
/// snapshot, then replay delta events since the snapshot round.
pub struct FastSyncManager {
    /// Our own node ID.
    #[allow(dead_code)] // Used for peer identification in future P2P sync
    node_id: NodeId,
    /// Whether fast sync is enabled.
    enabled: bool,
    /// Network interface for P2P operations.
    network: Option<Arc<dyn SyncNetwork>>,
}

impl std::fmt::Debug for FastSyncManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FastSyncManager")
            .field("node_id", &hex::encode(&self.node_id[..4]))
            .field("enabled", &self.enabled)
            .field("network", &self.network.as_ref().map(|_| "Some(SyncNetwork)"))
            .finish()
    }
}

impl FastSyncManager {
    /// Create a new fast-sync manager without a network interface.
    ///
    /// Without a network, [`sync_to_latest`](Self::sync_to_latest) will
    /// return an error. Use [`with_network`](Self::with_network) to
    /// provide a network implementation.
    pub fn new(node_id: NodeId, enabled: bool) -> Self {
        Self {
            node_id,
            enabled,
            network: None,
        }
    }

    /// Create a new fast-sync manager with a network interface.
    ///
    /// The network implementation abstracts over the P2P layer (libp2p,
    /// mock, etc.), allowing the manager to query peers and download
    /// snapshots.
    pub fn with_network(node_id: NodeId, enabled: bool, network: Arc<dyn SyncNetwork>) -> Self {
        Self {
            node_id,
            enabled,
            network: Some(network),
        }
    }

    /// Check if fast sync is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Create a checkpoint from the current state.
    pub fn create_checkpoint(
        round: u64,
        state_root: [u8; 32],
        snapshot_data: &[u8],
        event_count: u64,
    ) -> SyncCheckpoint {
        let snapshot_hash = blake3_hash_domain(b"OMNIA-FAST-SYNC-V1", snapshot_data);
        SyncCheckpoint {
            round,
            state_root,
            snapshot_hash,
            event_count,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            peer_id: None,
        }
    }

    /// Full fast-sync: query peers → download snapshot → verify → apply → replay delta.
    ///
    /// # Steps
    ///
    /// 1. Query all connected peers for their latest checkpoint
    /// 2. Select target checkpoint via supermajority agreement
    /// 3. Download the snapshot from the target peer
    /// 4. Verify snapshot integrity (BLAKE3 domain-separated hash)
    /// 5. Deserialize the snapshot
    /// 6. Download and replay delta events since the snapshot round
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::Network`] if no network is configured,
    /// [`SyncError::NoPeersAvailable`] if no peers have checkpoints,
    /// [`SyncError::IntegrityCheckFailed`] if the snapshot is corrupt,
    /// and other variants for protocol-level failures.
    #[allow(unreachable_code)] // TODO: Remove when snapshot application is implemented
    pub async fn sync_to_latest(&self) -> Result<SyncResult, SyncError> {
        let network = self
            .network
            .as_ref()
            .ok_or_else(|| SyncError::Network("No network configured".to_string()))?;

        // Step 1: Query connected peers for their latest checkpoint
        let checkpoints = self.query_peer_checkpoints(network)?;
        if checkpoints.is_empty() {
            return Err(SyncError::NoPeersAvailable);
        }

        // Step 2: Select target checkpoint (supermajority agreement)
        let total_peers = network.connected_peers().len();
        let target = select_target_checkpoint(&checkpoints, total_peers)?;

        // Step 3: Download snapshot from the target peer
        let peer_id = target
            .peer_id
            .ok_or_else(|| SyncError::Network("Checkpoint has no peer ID".to_string()))?;
        let snapshot_data = self.download_snapshot_from_peer(network, peer_id, &target)?;

        // Step 4: Verify snapshot integrity
        target.verify_snapshot(&snapshot_data)?;

        // Step 5: Deserialize snapshot
        let _snapshot: SyncSnapshot = postcard::from_bytes(&snapshot_data)
            .map_err(|e| SyncError::Consensus(format!("Snapshot deserialization failed: {e}")))?;

        // TODO: Apply snapshot to local state. This requires:
        // 1. Replace local CausalGraph with snapshot.causal_graph_data
        // 2. Reset ConsensusEngine state with snapshot.consensus_data
        // 3. Apply delta events on top of the snapshot
        // For now, return an error indicating this is not yet implemented.
        return Err(SyncError::Consensus(
            "Fast-sync snapshot application not yet implemented. Use full sync instead.".to_string(),
        ));

        // Step 6: Download and replay delta events
        let delta_events = self.download_delta_events(network, peer_id, target.round)?;

        tracing::info!(
            synced_to_round = target.round,
            events_replayed = delta_events.len(),
            snapshot_hash = ?&target.snapshot_hash[..8],
            "Fast-sync completed"
        );

        Ok(SyncResult {
            synced_to_round: target.round,
            events_replayed: delta_events.len() as u64,
            snapshot_hash: target.snapshot_hash,
        })
    }

    /// Attempt fast-sync, returning a result for the caller to decide
    /// whether to fall back to genesis replay.
    ///
    /// On success, returns the [`SyncResult`] from the sync operation.
    /// On failure, logs a warning and returns a zero [`SyncResult`]
    /// indicating no sync happened, so the caller can fall back to
    /// replaying events from genesis.
    pub async fn try_sync_or_fallback(&self) -> SyncResult {
        match self.sync_to_latest().await {
            Ok(result) => {
                tracing::info!(
                    synced_to_round = result.synced_to_round,
                    events_replayed = result.events_replayed,
                    "Fast-sync completed successfully"
                );
                result
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Fast-sync failed, will fall back to genesis replay"
                );
                // Return a zero result indicating no sync happened
                SyncResult {
                    synced_to_round: 0,
                    events_replayed: 0,
                    snapshot_hash: [0u8; 32],
                }
            }
        }
    }

    /// Query all connected peers for their latest checkpoint.
    fn query_peer_checkpoints(&self, network: &Arc<dyn SyncNetwork>) -> Result<Vec<SyncCheckpoint>, SyncError> {
        let peers = network.connected_peers();
        let mut checkpoints = Vec::new();

        for peer_id in peers {
            match network.send_request(peer_id, SyncRequest::GetCheckpoint) {
                Ok(SyncResponse::Checkpoint(Some(cp))) => checkpoints.push(cp),
                Ok(SyncResponse::Checkpoint(None)) => {
                    tracing::debug!(peer = ?&peer_id[..4], "Peer has no checkpoint");
                }
                Ok(_) => {
                    tracing::debug!(peer = ?&peer_id[..4], "Unexpected response type");
                }
                Err(e) => {
                    tracing::warn!(
                        peer = ?&peer_id[..4],
                        error = %e,
                        "Failed to query peer checkpoint"
                    );
                }
            }
        }

        Ok(checkpoints)
    }

    /// Download a snapshot from a specific peer.
    fn download_snapshot_from_peer(
        &self,
        network: &Arc<dyn SyncNetwork>,
        peer_id: NodeId,
        checkpoint: &SyncCheckpoint,
    ) -> Result<Vec<u8>, SyncError> {
        match network.send_request(
            peer_id,
            SyncRequest::GetSnapshot {
                checkpoint_hash: checkpoint.snapshot_hash,
            },
        ) {
            Ok(SyncResponse::Snapshot(Some(data))) => {
                if data.len() > MAX_SNAPSHOT_SIZE {
                    return Err(SyncError::Consensus(format!(
                        "Snapshot size {} exceeds maximum {}",
                        data.len(),
                        MAX_SNAPSHOT_SIZE
                    )));
                }
                Ok(data)
            }
            Ok(SyncResponse::Snapshot(None)) => Err(SyncError::Network("Peer returned no snapshot data".to_string())),
            Ok(_) => Err(SyncError::Network("Unexpected response type".to_string())),
            Err(e) => Err(e),
        }
    }

    /// Download delta events from a peer starting at a given round.
    fn download_delta_events(
        &self,
        network: &Arc<dyn SyncNetwork>,
        peer_id: NodeId,
        from_round: u64,
    ) -> Result<Vec<Vec<u8>>, SyncError> {
        let effective_to_round = from_round.saturating_add(MAX_SYNC_ROUNDS);
        match network.send_request(
            peer_id,
            SyncRequest::GetEvents {
                from_round,
                to_round: effective_to_round,
            },
        ) {
            Ok(SyncResponse::Events(events)) => {
                // Validate event count limit
                if events.len() > MAX_SYNC_EVENTS_COUNT {
                    return Err(SyncError::Consensus(format!(
                        "Too many events in sync response: {} (max {})",
                        events.len(),
                        MAX_SYNC_EVENTS_COUNT
                    )));
                }
                // Validate total byte size limit
                let total_bytes: usize = events.iter().map(|e| e.len()).sum();
                if total_bytes > MAX_SYNC_EVENTS_TOTAL_BYTES {
                    return Err(SyncError::Consensus(format!(
                        "Total event bytes in sync response: {} (max {})",
                        total_bytes,
                        MAX_SYNC_EVENTS_TOTAL_BYTES
                    )));
                }
                Ok(events)
            }
            Ok(_) => Err(SyncError::Network("Unexpected response type".to_string())),
            Err(e) => Err(e),
        }
    }
}

/// Mock network implementation for testing fast-sync.
#[cfg(test)]
pub struct MockSyncNetwork {
    peers: Vec<NodeId>,
    checkpoint: Option<SyncCheckpoint>,
    snapshot_data: Option<Vec<u8>>,
    delta_events: Vec<Vec<u8>>,
}

#[cfg(test)]
impl MockSyncNetwork {
    /// Create a new empty mock network.
    pub fn new() -> Self {
        Self {
            peers: Vec::new(),
            checkpoint: None,
            snapshot_data: None,
            delta_events: Vec::new(),
        }
    }

    /// Add a peer to the mock network.
    pub fn with_peer(mut self, peer_id: NodeId) -> Self {
        self.peers.push(peer_id);
        self
    }

    /// Set the checkpoint that peers will return.
    pub fn with_checkpoint(mut self, cp: impl Into<Option<SyncCheckpoint>>) -> Self {
        self.checkpoint = cp.into();
        self
    }

    /// Set the snapshot data that peers will return.
    pub fn with_snapshot(mut self, data: Vec<u8>) -> Self {
        self.snapshot_data = Some(data);
        self
    }

    /// Set the delta events that peers will return.
    pub fn with_delta_events(mut self, events: Vec<Vec<u8>>) -> Self {
        self.delta_events = events;
        self
    }
}

#[cfg(test)]
impl Default for MockSyncNetwork {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl SyncNetwork for MockSyncNetwork {
    fn connected_peers(&self) -> Vec<NodeId> {
        self.peers.clone()
    }

    fn send_request(&self, _peer_id: NodeId, request: SyncRequest) -> Result<SyncResponse, SyncError> {
        match request {
            SyncRequest::GetCheckpoint => Ok(SyncResponse::Checkpoint(self.checkpoint.clone())),
            SyncRequest::GetSnapshot { .. } => Ok(SyncResponse::Snapshot(self.snapshot_data.clone())),
            SyncRequest::GetEvents { .. } => Ok(SyncResponse::Events(self.delta_events.clone())),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    /// Create a valid checkpoint + snapshot data pair for testing.
    ///
    /// The snapshot data is a properly serialized [`SyncSnapshot`] whose
    /// BLAKE3 hash matches the checkpoint's `snapshot_hash`.
    fn make_checkpoint_and_snapshot(round: u64, peer_id: NodeId) -> (SyncCheckpoint, Vec<u8>) {
        let snapshot = SyncSnapshot {
            round,
            state_root: [1u8; 32],
            causal_graph_data: vec![0xAA, 0xBB],
            consensus_data: vec![0xCC, 0xDD],
            event_count: round * 100,
        };
        let snapshot_data = postcard::to_allocvec(&snapshot).expect("snapshot serialization should not fail");
        let snapshot_hash = blake3_hash_domain(b"OMNIA-FAST-SYNC-V1", &snapshot_data);

        let checkpoint = SyncCheckpoint {
            round,
            state_root: [1u8; 32],
            snapshot_hash,
            event_count: round * 100,
            timestamp: 0,
            peer_id: Some(peer_id),
        };

        (checkpoint, snapshot_data)
    }

    #[test]
    fn test_sync_checkpoint_verification() {
        let data = b"test snapshot data";
        let cp = FastSyncManager::create_checkpoint(10, [1u8; 32], data, 100);
        assert!(cp.verify_snapshot(data).is_ok());

        let bad_data = b"tampered snapshot";
        assert!(cp.verify_snapshot(bad_data).is_err());
    }

    #[test]
    fn test_sync_checkpoint_selection() {
        let make_cp = |round: u64, root: [u8; 32]| SyncCheckpoint {
            round,
            state_root: root,
            snapshot_hash: [0u8; 32],
            event_count: round * 100,
            timestamp: 0,
            peer_id: None,
        };

        // 4 peers agree on round 100
        let checkpoints: Vec<SyncCheckpoint> = (0..4)
            .map(|_| make_cp(100, [1u8; 32]))
            .chain(std::iter::once(make_cp(50, [2u8; 32]))) // 1 outlier
            .collect();

        let result = select_target_checkpoint(&checkpoints, 5);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().round, 100);
    }

    #[test]
    fn test_snapshot_integrity_verification() {
        let data = b"snapshot data for integrity check";
        let cp = FastSyncManager::create_checkpoint(5, [0u8; 32], data, 50);

        // Valid
        assert!(cp.verify_snapshot(data).is_ok());

        // Tampered
        let mut tampered = data.to_vec();
        tampered[0] ^= 0xFF;
        assert!(cp.verify_snapshot(&tampered).is_err());
    }

    #[test]
    fn test_sync_error_no_peers() {
        let checkpoints: Vec<SyncCheckpoint> = vec![];
        let result = select_target_checkpoint(&checkpoints, 5);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SyncError::NoPeersAvailable));
    }

    #[test]
    fn test_sync_error_insufficient_agreement() {
        // All 5 peers disagree
        let checkpoints: Vec<SyncCheckpoint> = (0..5)
            .map(|i| {
                let mut root = [0u8; 32];
                root[0] = i;
                SyncCheckpoint {
                    round: i as u64 * 10,
                    state_root: root,
                    snapshot_hash: [0u8; 32],
                    event_count: 0,
                    timestamp: 0,
                    peer_id: None,
                }
            })
            .collect();

        let result = select_target_checkpoint(&checkpoints, 5);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SyncError::InsufficientAgreement { .. }));
    }

    #[test]
    fn test_fast_sync_manager_creation() {
        let manager = FastSyncManager::new(test_node(1), true);
        assert!(manager.is_enabled());

        let manager_disabled = FastSyncManager::new(test_node(2), false);
        assert!(!manager_disabled.is_enabled());
    }

    #[test]
    fn test_checkpoint_timestamp() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let cp = FastSyncManager::create_checkpoint(1, [0u8; 32], b"data", 0);

        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        assert!(cp.timestamp >= before);
        assert!(cp.timestamp <= after);
    }

    #[test]
    fn test_sync_request_response_serialization() {
        // Round-trip SyncRequest
        let req = SyncRequest::GetCheckpoint;
        let bytes = postcard::to_allocvec(&req).unwrap();
        let decoded: SyncRequest = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded, SyncRequest::GetCheckpoint));

        let req = SyncRequest::GetSnapshot {
            checkpoint_hash: [42u8; 32],
        };
        let bytes = postcard::to_allocvec(&req).unwrap();
        let decoded: SyncRequest = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded, SyncRequest::GetSnapshot { .. }));

        let req = SyncRequest::GetEvents {
            from_round: 10,
            to_round: 20,
        };
        let bytes = postcard::to_allocvec(&req).unwrap();
        let decoded: SyncRequest = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded, SyncRequest::GetEvents { .. }));

        // Round-trip SyncResponse
        let resp = SyncResponse::Checkpoint(None);
        let bytes = postcard::to_allocvec(&resp).unwrap();
        let decoded: SyncResponse = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded, SyncResponse::Checkpoint(None)));

        let resp = SyncResponse::Snapshot(Some(vec![1, 2, 3]));
        let bytes = postcard::to_allocvec(&resp).unwrap();
        let decoded: SyncResponse = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded, SyncResponse::Snapshot(Some(_))));

        let resp = SyncResponse::Events(vec![vec![4, 5, 6]]);
        let bytes = postcard::to_allocvec(&resp).unwrap();
        let decoded: SyncResponse = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded, SyncResponse::Events(_)));
    }

    #[test]
    fn test_sync_result_serialization() {
        let result = SyncResult {
            synced_to_round: 42,
            events_replayed: 7,
            snapshot_hash: [99u8; 32],
        };
        let bytes = postcard::to_allocvec(&result).unwrap();
        let decoded: SyncResult = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.synced_to_round, 42);
        assert_eq!(decoded.events_replayed, 7);
        assert_eq!(decoded.snapshot_hash, [99u8; 32]);
    }

    #[test]
    fn test_select_highest_round_with_supermajority() {
        let make_cp = |round: u64, root: [u8; 32]| SyncCheckpoint {
            round,
            state_root: root,
            snapshot_hash: [0u8; 32],
            event_count: round * 100,
            timestamp: 0,
            peer_id: None,
        };

        // 3 peers agree on round 200, 4 peers agree on round 100
        // Supermajority for 7 peers = (2*7/3) + 1 = 5
        // Neither group has supermajority alone
        // Let's try with 5 agree on round 200, 2 on round 100
        let checkpoints: Vec<SyncCheckpoint> = (0..5)
            .map(|_| make_cp(200, [1u8; 32]))
            .chain((0..2).map(|_| make_cp(100, [2u8; 32])))
            .collect();

        let result = select_target_checkpoint(&checkpoints, 7);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().round, 200);
    }

    // ── New tests for H-3: Fast-Sync P2P Automation ──────────────────

    #[tokio::test]
    async fn test_fast_sync_full_loop() {
        let peer_id = test_node(10);

        // Create a checkpoint + valid snapshot data
        let (checkpoint, snapshot_data) = make_checkpoint_and_snapshot(500, peer_id);

        // Create a mock network with the peer, checkpoint, snapshot, and delta events
        let delta_events: Vec<Vec<u8>> = vec![vec![1, 2, 3], vec![4, 5, 6]];
        let network = MockSyncNetwork::new()
            .with_peer(peer_id)
            .with_checkpoint(checkpoint.clone())
            .with_snapshot(snapshot_data)
            .with_delta_events(delta_events);

        let manager = FastSyncManager::with_network(test_node(1), true, Arc::new(network));

        let result = manager.sync_to_latest().await;
        // Snapshot application is not yet implemented, so sync should return an error
        assert!(
            result.is_err(),
            "Full sync loop should fail until snapshot application is implemented"
        );
        match result.unwrap_err() {
            SyncError::Consensus(msg) => {
                assert!(
                    msg.contains("not yet implemented"),
                    "Expected 'not yet implemented' error, got: {msg}"
                );
            }
            other => panic!("Expected SyncError::Consensus, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_fast_sync_no_network() {
        // FastSyncManager without network → error
        let manager = FastSyncManager::new(test_node(1), true);

        let result = manager.sync_to_latest().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SyncError::Network(msg) => {
                assert!(
                    msg.contains("No network configured"),
                    "Expected 'No network configured' error, got: {msg}"
                );
            }
            other => panic!("Expected SyncError::Network, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_fast_sync_no_peers() {
        // No peers available → NoPeersAvailable error
        let network = MockSyncNetwork::new(); // No peers, no checkpoint
        let manager = FastSyncManager::with_network(test_node(1), true, Arc::new(network));

        let result = manager.sync_to_latest().await;
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), SyncError::NoPeersAvailable),
            "Expected NoPeersAvailable error"
        );
    }

    #[tokio::test]
    async fn test_fast_sync_integrity_check() {
        let peer_id = test_node(10);

        // Create a valid checkpoint
        let (checkpoint, _valid_snapshot_data) = make_checkpoint_and_snapshot(100, peer_id);

        // Provide tampered snapshot data (doesn't match checkpoint hash)
        let tampered_data = b"tampered snapshot data that does not match hash".to_vec();

        let network = MockSyncNetwork::new()
            .with_peer(peer_id)
            .with_checkpoint(checkpoint)
            .with_snapshot(tampered_data);

        let manager = FastSyncManager::with_network(test_node(1), true, Arc::new(network));

        let result = manager.sync_to_latest().await;
        assert!(result.is_err(), "Tampered snapshot should fail integrity check");
        match result.unwrap_err() {
            SyncError::IntegrityCheckFailed { expected, actual } => {
                assert_ne!(expected, actual, "Expected and actual hashes should differ");
            }
            other => panic!("Expected IntegrityCheckFailed, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_fast_sync_fallback_on_failure() {
        // No network → sync fails → fallback returns zero result
        let manager = FastSyncManager::new(test_node(1), true);

        let result = manager.try_sync_or_fallback().await;
        assert_eq!(result.synced_to_round, 0);
        assert_eq!(result.events_replayed, 0);
        assert_eq!(result.snapshot_hash, [0u8; 32]);
    }

    #[tokio::test]
    async fn test_fast_sync_fallback_on_success() {
        let peer_id = test_node(10);
        let (checkpoint, snapshot_data) = make_checkpoint_and_snapshot(300, peer_id);
        let delta_events: Vec<Vec<u8>> = vec![vec![7, 8, 9]];

        let network = MockSyncNetwork::new()
            .with_peer(peer_id)
            .with_checkpoint(checkpoint.clone())
            .with_snapshot(snapshot_data)
            .with_delta_events(delta_events);

        let manager = FastSyncManager::with_network(test_node(1), true, Arc::new(network));

        // Snapshot application is not yet implemented, so try_sync_or_fallback
        // will catch the Consensus error and return a zero result.
        let result = manager.try_sync_or_fallback().await;
        assert_eq!(result.synced_to_round, 0);
        assert_eq!(result.events_replayed, 0);
    }

    #[tokio::test]
    async fn test_fast_sync_peer_with_no_checkpoint() {
        // Peer exists but has no checkpoint → NoPeersAvailable
        let peer_id = test_node(10);
        let network = MockSyncNetwork::new().with_peer(peer_id);
        // No with_checkpoint() call — defaults to None

        let manager = FastSyncManager::with_network(test_node(1), true, Arc::new(network));

        // Need multiple peers for supermajority — but even with one peer,
        // if it has no checkpoint, we get NoPeersAvailable
        let result = manager.sync_to_latest().await;
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), SyncError::NoPeersAvailable),
            "Expected NoPeersAvailable when peer has no checkpoint"
        );
    }

    #[tokio::test]
    async fn test_fast_sync_insufficient_agreement() {
        // Since MockSyncNetwork returns the same data to all peers,
        // test the InsufficientAgreement path via select_target_checkpoint
        // directly with disagreeing checkpoints.
        let checkpoints: Vec<SyncCheckpoint> = (0..3)
            .map(|i| {
                let mut root = [0u8; 32];
                root[0] = i as u8;
                SyncCheckpoint {
                    round: (i as u64 + 1) * 100,
                    state_root: root,
                    snapshot_hash: [0u8; 32],
                    event_count: 0,
                    timestamp: 0,
                    peer_id: Some(test_node(10 + i as u8)),
                }
            })
            .collect();

        let result = select_target_checkpoint(&checkpoints, 3);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), SyncError::InsufficientAgreement { .. }),
            "Expected InsufficientAgreement when all peers disagree"
        );
    }

    #[test]
    fn test_sync_snapshot_serialization() {
        let snapshot = SyncSnapshot {
            round: 42,
            state_root: [7u8; 32],
            causal_graph_data: vec![1, 2, 3],
            consensus_data: vec![4, 5, 6],
            event_count: 100,
        };

        let bytes = postcard::to_allocvec(&snapshot).unwrap();
        let decoded: SyncSnapshot = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.round, 42);
        assert_eq!(decoded.state_root, [7u8; 32]);
        assert_eq!(decoded.causal_graph_data, vec![1, 2, 3]);
        assert_eq!(decoded.consensus_data, vec![4, 5, 6]);
        assert_eq!(decoded.event_count, 100);
    }

    #[test]
    fn test_with_network_constructor() {
        let network = MockSyncNetwork::new().with_peer(test_node(5));
        let manager = FastSyncManager::with_network(test_node(1), true, Arc::new(network));

        assert!(manager.is_enabled());
    }

    #[test]
    fn test_mock_sync_network_default() {
        let network = MockSyncNetwork::default();
        assert!(network.connected_peers().is_empty());
    }
}
