//! Fast-sync protocol for late-joining nodes.
//!
//! Instead of replaying all events from genesis, new nodes can:
//! 1. Download a recent state snapshot from peers
//! 2. Verify snapshot integrity via BLAKE3
//! 3. Replay only the delta events since the snapshot

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::blake3_domain::blake3_hash_domain;
use crate::NodeId;

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
    let supermajority = (2 * total_peers / 3) + 1;
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
#[derive(Debug)]
pub struct FastSyncManager {
    /// Our own node ID.
    #[allow(dead_code)] // Used for peer identification in future P2P sync
    node_id: NodeId,
    /// Whether fast sync is enabled.
    enabled: bool,
}

impl FastSyncManager {
    /// Create a new fast-sync manager.
    pub fn new(node_id: NodeId, enabled: bool) -> Self {
        Self { node_id, enabled }
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
        assert!(matches!(
            result.unwrap_err(),
            SyncError::InsufficientAgreement { .. }
        ));
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
}
