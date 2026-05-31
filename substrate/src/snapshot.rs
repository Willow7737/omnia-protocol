//! State snapshot system for fast node synchronization.
//!
//! Instead of replaying all events from genesis, new nodes can import
//! a recent snapshot and only sync events after the snapshot height.
//!
//! # Snapshot Format
//!
//! A [`StateSnapshot`] contains:
//! - The causal graph (serialized via [`GraphSnapshot`])
//! - The slashing state (serialized via [`SlashingState`])
//! - The nonce map (per-creator last nonce)
//! - An integrity hash (BLAKE3 over all serialized components)
//!
//! # Usage
//!
//! ```ignore
//! use omnia_substrate::snapshot::StateSnapshot;
//! use std::path::Path;
//!
//! // Take a snapshot
//! let snapshot = StateSnapshot::take(&graph, &slashing, &nonces, height)?;
//!
//! // Verify integrity
//! snapshot.verify()?;
//!
//! // Write to disk
//! snapshot.write_to_file(Path::new("snapshot.bin"))?;
//!
//! // Restore from disk
//! let restored = StateSnapshot::read_from_file(Path::new("snapshot.bin"))?;
//! restored.verify()?;
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::causal_graph::CausalGraph;
use crate::causal_graph::GraphSnapshot;
use crate::slashing::SlashingState;

/// Current snapshot format version.
const SNAPSHOT_VERSION: u32 = 1;

/// Maximum allowed snapshot size during deserialization (64 MiB).
///
/// Prevents memory-exhaustion attacks where a crafted snapshot
/// causes the node to allocate an arbitrarily large buffer.
const MAX_SNAPSHOT_SIZE: usize = 64 * 1024 * 1024;

/// A complete state snapshot.
///
/// Contains all the state needed to reconstruct a node at a given height,
/// plus integrity checks to detect corruption.
#[derive(Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Snapshot format version.
    pub version: u32,
    /// The event height at which this snapshot was taken.
    pub height: u64,
    /// Total events in the causal graph at snapshot time.
    pub event_count: u64,
    /// Timestamp (unix epoch seconds).
    pub timestamp: u64,
    /// BLAKE3 hash of all serialized state components (integrity check).
    pub state_root: [u8; 32],
    /// Serialized [`GraphSnapshot`].
    pub causal_graph_bytes: Vec<u8>,
    /// Serialized [`SlashingState`].
    pub slashing_state_bytes: Vec<u8>,
    /// Serialized nonce map.
    pub nonce_state_bytes: Vec<u8>,
}

/// Errors during snapshot operations.
#[derive(Error, Debug)]
pub enum SnapshotError {
    /// Snapshot integrity check failed.
    #[error("snapshot integrity check failed: computed {computed}, stored {stored}")]
    IntegrityCheckFailed {
        /// The recomputed state root.
        computed: String,
        /// The stored state root.
        stored: String,
    },
    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),
    /// I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Unsupported snapshot version.
    #[error("unsupported snapshot version: {0}")]
    UnsupportedVersion(u32),
}

impl StateSnapshot {
    /// Create a snapshot from the current node state.
    ///
    /// # Arguments
    ///
    /// * `graph` — The causal graph to snapshot
    /// * `slashing` — The slashing state to snapshot
    /// * `nonces` — The nonce map to snapshot
    /// * `height` — The current event height
    ///
    /// # Returns
    ///
    /// A new [`StateSnapshot`] with computed integrity hash.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Serialization`] if any component fails to
    /// serialize.
    pub fn take(
        graph: &CausalGraph,
        slashing: &SlashingState,
        nonces: &HashMap<[u8; 32], u64>,
        height: u64,
    ) -> Result<Self, SnapshotError> {
        let causal_graph_bytes = postcard::to_allocvec(&GraphSnapshot::from(graph))
            .map_err(|e| SnapshotError::Serialization(e.to_string()))?;
        let slashing_state_bytes =
            postcard::to_allocvec(slashing).map_err(|e| SnapshotError::Serialization(e.to_string()))?;
        let nonce_state_bytes =
            postcard::to_allocvec(nonces).map_err(|e| SnapshotError::Serialization(e.to_string()))?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Compute state root using BLAKE3 for integrity verification.
        // BLAKE3 is chosen for its speed and security (RFC 7693).
        let mut hasher = blake3::Hasher::new();
        hasher.update(&causal_graph_bytes);
        hasher.update(&slashing_state_bytes);
        hasher.update(&nonce_state_bytes);
        let state_root = *hasher.finalize().as_bytes();

        let event_count = graph.len() as u64;

        Ok(Self {
            version: SNAPSHOT_VERSION,
            height,
            event_count,
            timestamp,
            state_root,
            causal_graph_bytes,
            slashing_state_bytes,
            nonce_state_bytes,
        })
    }

    /// Verify the snapshot's integrity (recompute `state_root` and compare).
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::IntegrityCheckFailed`] if the recomputed
    /// state root does not match the stored one.
    pub fn verify(&self) -> Result<(), SnapshotError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.causal_graph_bytes);
        hasher.update(&self.slashing_state_bytes);
        hasher.update(&self.nonce_state_bytes);
        let computed = *hasher.finalize().as_bytes();

        if computed != self.state_root {
            return Err(SnapshotError::IntegrityCheckFailed {
                computed: hex::encode(computed),
                stored: hex::encode(self.state_root),
            });
        }
        Ok(())
    }

    /// Serialize the snapshot to bytes.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Serialization`] if serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SnapshotError> {
        postcard::to_allocvec(self).map_err(|e| SnapshotError::Serialization(e.to_string()))
    }

    /// Deserialize a snapshot from bytes.
    ///
    /// Also validates the version field.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Serialization`] if deserialization fails,
    /// or [`SnapshotError::UnsupportedVersion`] if the version is not `1`.
    pub fn from_bytes(data: &[u8]) -> Result<Self, SnapshotError> {
        if data.len() > MAX_SNAPSHOT_SIZE {
            return Err(SnapshotError::Serialization(format!(
                "snapshot size {} bytes exceeds maximum {} bytes",
                data.len(),
                MAX_SNAPSHOT_SIZE
            )));
        }
        let snapshot: Self = postcard::from_bytes(data).map_err(|e| SnapshotError::Serialization(e.to_string()))?;
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::UnsupportedVersion(snapshot.version));
        }
        Ok(snapshot)
    }

    /// Write the snapshot to a file.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Io`] if the file cannot be written.
    pub fn write_to_file(&self, path: &Path) -> Result<(), SnapshotError> {
        let bytes = self.to_bytes()?;
        // Write to a temporary file first, then atomically rename.
        // This prevents corruption if the process crashes mid-write.
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Read a snapshot from a file.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Io`] if the file cannot be read,
    /// [`SnapshotError::Serialization`] if the data cannot be parsed,
    /// or [`SnapshotError::UnsupportedVersion`] if the version is wrong.
    pub fn read_from_file(path: &Path) -> Result<Self, SnapshotError> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::causal_graph::CausalGraph;
    use crate::crypto::generate_keypair;
    use crate::event::Event;
    use crate::slashing::SlashingState;
    use crate::vector_clock::VectorClock;

    fn test_node(id: u8) -> [u8; 32] {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn make_event(creator: [u8; 32], seq: u64) -> Event {
        let vc = VectorClock::with_node(creator, seq + 1);
        let mut event = Event::new(creator, seq, vc, None, None, vec![1, 2, 3]).expect("valid event");
        event.sign_with_keypair(&generate_keypair()).expect("signing");
        event
    }

    #[test]
    fn test_snapshot_take_and_verify_passes() {
        let mut graph = CausalGraph::new();
        let e = make_event(test_node(1), 0);
        graph.insert(e).unwrap();

        let slashing = SlashingState::default();
        let nonces = HashMap::new();

        let snapshot = StateSnapshot::take(&graph, &slashing, &nonces, 1).expect("take should succeed");

        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.height, 1);
        assert_eq!(snapshot.event_count, 1);

        snapshot
            .verify()
            .expect("verify should succeed for unmodified snapshot");
    }

    #[test]
    fn test_snapshot_modify_then_verify_fails() {
        let graph = CausalGraph::new();
        let slashing = SlashingState::default();
        let nonces = HashMap::new();

        let mut snapshot = StateSnapshot::take(&graph, &slashing, &nonces, 0).expect("take should succeed");

        // Tamper with the serialized data
        if !snapshot.causal_graph_bytes.is_empty() {
            snapshot.causal_graph_bytes[0] ^= 0xFF;
        } else {
            snapshot.causal_graph_bytes.push(42);
        }

        let result = snapshot.verify();
        assert!(result.is_err(), "verify should fail for modified snapshot");
        if let Err(SnapshotError::IntegrityCheckFailed { .. }) = result {
            // expected
        } else {
            panic!("expected IntegrityCheckFailed, got {result:?}");
        }
    }

    #[test]
    fn test_snapshot_serialize_deserialize_verify_passes() {
        let mut graph = CausalGraph::new();
        let e = make_event(test_node(1), 0);
        graph.insert(e).unwrap();

        let slashing = SlashingState::default();
        let mut nonces = HashMap::new();
        nonces.insert(test_node(1), 42);

        let snapshot = StateSnapshot::take(&graph, &slashing, &nonces, 1).expect("take should succeed");

        let bytes = snapshot.to_bytes().expect("to_bytes should succeed");
        let restored = StateSnapshot::from_bytes(&bytes).expect("from_bytes should succeed");

        assert_eq!(restored.version, snapshot.version);
        assert_eq!(restored.height, snapshot.height);
        assert_eq!(restored.event_count, snapshot.event_count);
        assert_eq!(restored.state_root, snapshot.state_root);

        restored.verify().expect("restored snapshot should verify");
    }

    #[test]
    fn test_snapshot_unsupported_version_rejected() {
        let graph = CausalGraph::new();
        let slashing = SlashingState::default();
        let nonces = HashMap::new();

        let mut snapshot = StateSnapshot::take(&graph, &slashing, &nonces, 0).expect("take should succeed");

        // Manually corrupt the version
        snapshot.version = 999;
        let bytes = postcard::to_allocvec(&snapshot).expect("serialize");
        let result = StateSnapshot::from_bytes(&bytes);
        assert!(
            matches!(result, Err(SnapshotError::UnsupportedVersion(999))),
            "unsupported version should be rejected"
        );
    }
}
