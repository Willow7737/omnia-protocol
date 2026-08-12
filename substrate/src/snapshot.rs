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
//! # P2P Wire Bridge
//!
//! [`StateSnapshot`] is the **local** format (disk, same-process).
//! [`omnia_network::fast_sync::SyncSnapshot`] is the **P2P wire** format.
//! They carry the same logical data but with different blob layouts:
//!
//! | Data                | `StateSnapshot`               | `SyncSnapshot`                    |
//! |---------------------|-------------------------------|-----------------------------------|
//! | Round/height        | `height`                     | `round`                           |
//! | State root          | `state_root`                 | `state_root`                      |
//! | Causal graph        | `causal_graph_bytes`         | `causal_graph_data`               |
//! | Slashing + nonces    | `slashing_state_bytes` +     | `consensus_data` (packed envelope |
//! |                     | `nonce_state_bytes`           |  via [`SyncConsensusEnvelope`])   |
//! | Event count         | `event_count`                | `event_count`                     |
//!
//! Use [`StateSnapshot::from_sync_snapshot`] and
//! [`StateSnapshot::into_sync_snapshot`] to convert between them.
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

/// Envelope for packing slashing state + nonce map into
/// [`SyncSnapshot::consensus_data`].
///
/// The P2P wire format (`SyncSnapshot`) has a single opaque
/// `consensus_data: Vec<u8>` blob, but the local format (`StateSnapshot`)
/// stores slashing and nonces as separate blobs. This envelope
/// defines the serialization contract so both sides agree on the
/// layout.
///
/// # Wire layout (postcard)
///
/// ```text
/// [u32: envelope_version]  -- must be 1
/// [Vec<u8>: slashing_state_bytes]
/// [Vec<u8>: nonce_state_bytes]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConsensusEnvelope {
    /// Envelope format version.
    pub envelope_version: u32,
    /// Serialized [`SlashingState`].
    pub slashing_state_bytes: Vec<u8>,
    /// Serialized nonce map (`HashMap<NodeId, u64>`).
    pub nonce_state_bytes: Vec<u8>,
}

impl SyncConsensusEnvelope {
    /// Current envelope version.
    pub const VERSION: u32 = 1;

    /// Pack slashing state and nonces into a single blob for
    /// `SyncSnapshot::consensus_data`.
    pub fn pack(
        slashing_state_bytes: Vec<u8>,
        nonce_state_bytes: Vec<u8>,
    ) -> Result<Vec<u8>, SnapshotError> {
        let envelope = Self {
            envelope_version: Self::VERSION,
            slashing_state_bytes,
            nonce_state_bytes,
        };
        postcard::to_allocvec(&envelope)
            .map_err(|e| SnapshotError::Serialization(format!("consensus envelope: {e}")))
    }

    /// Unpack `SyncSnapshot::consensus_data` back into its components.
    pub fn unpack(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), SnapshotError> {
        let envelope: Self = postcard::from_bytes(data).map_err(|e| {
            SnapshotError::Serialization(format!("consensus envelope deserialization: {e}"))
        })?;
        if envelope.envelope_version != Self::VERSION {
            return Err(SnapshotError::UnsupportedVersion(envelope.envelope_version));
        }
        Ok((envelope.slashing_state_bytes, envelope.nonce_state_bytes))
    }
}

// ---------------------------------------------------------------------------
// P2P wire bridge
// ---------------------------------------------------------------------------

impl StateSnapshot {
    /// Convert from a P2P wire [`SyncSnapshot`].
    ///
    /// Unpacks the opaque `consensus_data` blob into slashing state
    /// and nonce components using [`SyncConsensusEnvelope`].
    ///
    /// # Integrity
    ///
    /// Does **not** call [`Self::verify`] — the caller is responsible
    /// for verifying integrity after conversion (the P2P layer has
    /// already checked the BLAKE3 snapshot hash via
    /// `SyncCheckpoint::verify_snapshot`).
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Serialization`] if the consensus
    /// envelope cannot be deserialized, or
    /// [`SnapshotError::UnsupportedVersion`] if the envelope version
    /// is not recognized.
    #[cfg(feature = "network")]
    pub fn from_sync_snapshot(
        sync: &omnia_network::fast_sync::SyncSnapshot,
    ) -> Result<Self, SnapshotError> {
        let (slashing_state_bytes, nonce_state_bytes) =
            SyncConsensusEnvelope::unpack(&sync.consensus_data)?;

        Ok(Self {
            version: SNAPSHOT_VERSION,
            height: sync.round,
            event_count: sync.event_count,
            timestamp: 0, // Not carried on the wire; set by local snapshot taker
            state_root: sync.state_root,
            causal_graph_bytes: sync.causal_graph_data.clone(),
            slashing_state_bytes,
            nonce_state_bytes,
        })
    }

    /// Convert into a P2P wire [`SyncSnapshot`].
    ///
    /// Packs slashing state and nonces into a single
    /// `consensus_data` blob using [`SyncConsensusEnvelope`].
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Serialization`] if the consensus
    /// envelope cannot be serialized.
    #[cfg(feature = "network")]
    pub fn into_sync_snapshot(
        &self,
    ) -> Result<omnia_network::fast_sync::SyncSnapshot, SnapshotError> {
        let consensus_data =
            SyncConsensusEnvelope::pack(
                self.slashing_state_bytes.clone(),
                self.nonce_state_bytes.clone(),
            )?;

        Ok(omnia_network::fast_sync::SyncSnapshot {
            round: self.height,
            state_root: self.state_root,
            causal_graph_data: self.causal_graph_bytes.clone(),
            consensus_data,
            event_count: self.event_count,
        })
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

    // ── SyncConsensusEnvelope tests ─────────────────────────────

    #[test]
    fn test_envelope_pack_unpack_round_trip() {
        let slashing = vec![0x01, 0x02, 0x03];
        let nonces = vec![0xAA, 0xBB, 0xCC, 0xDD];

        let packed = SyncConsensusEnvelope::pack(slashing.clone(), nonces.clone()).unwrap();
        let (out_slashing, out_nonces) = SyncConsensusEnvelope::unpack(&packed).unwrap();

        assert_eq!(out_slashing, slashing);
        assert_eq!(out_nonces, nonces);
    }

    #[test]
    fn test_envelope_rejects_bad_version() {
        // Serialize an envelope with version 99, then unpack should reject
        let bad = SyncConsensusEnvelope {
            envelope_version: 99,
            slashing_state_bytes: vec![1],
            nonce_state_bytes: vec![2],
        };
        let bytes = postcard::to_allocvec(&bad).unwrap();
        let result = SyncConsensusEnvelope::unpack(&bytes);
        assert!(
            matches!(result, Err(SnapshotError::UnsupportedVersion(99))),
            "bad envelope version should be rejected"
        );
    }

    #[test]
    fn test_envelope_rejects_garbage() {
        let result = SyncConsensusEnvelope::unpack(&[0xFF, 0xFE, 0xFD]);
        assert!(result.is_err(), "garbage bytes should fail deserialization");
    }

    // ── P2P bridge tests (requires `network` feature) ───────────

    #[cfg(feature = "network")]
    #[test]
    fn test_state_to_sync_to_state_round_trip() {
        use omnia_network::fast_sync::SyncSnapshot;

        let mut graph = CausalGraph::new();
        let e = make_event(test_node(1), 0);
        graph.insert(e).unwrap();

        let slashing = SlashingState::default();
        let mut nonces = HashMap::new();
        nonces.insert(test_node(1), 42);

        // Take a real StateSnapshot
        let state = StateSnapshot::take(&graph, &slashing, &nonces, 7).unwrap();

        // Convert to P2P wire format
        let sync: SyncSnapshot = state.into_sync_snapshot().unwrap();
        assert_eq!(sync.round, 7);
        assert_eq!(sync.event_count, 1);
        assert_eq!(sync.state_root, state.state_root);
        assert!(!sync.causal_graph_data.is_empty());
        assert!(!sync.consensus_data.is_empty());

        // Convert back to StateSnapshot
        let restored = StateSnapshot::from_sync_snapshot(&sync).unwrap();
        assert_eq!(restored.height, 7);
        assert_eq!(restored.event_count, 1);
        assert_eq!(restored.state_root, state.state_root);
        assert_eq!(restored.causal_graph_bytes, state.causal_graph_bytes);
        assert_eq!(restored.slashing_state_bytes, state.slashing_state_bytes);
        assert_eq!(restored.nonce_state_bytes, state.nonce_state_bytes);

        // Restored snapshot should pass its own integrity check
        restored.verify().unwrap();
    }

    #[cfg(feature = "network")]
    #[test]
    fn test_sync_with_nonces_survives_round_trip() {
        use omnia_network::fast_sync::SyncSnapshot;

        let graph = CausalGraph::new();
        let slashing = SlashingState::default();
        let mut nonces = HashMap::new();
        nonces.insert(test_node(1), 100);
        nonces.insert(test_node(2), 200);
        nonces.insert(test_node(3), 300);

        let state = StateSnapshot::take(&graph, &slashing, &nonces, 0).unwrap();
        let sync: SyncSnapshot = state.into_sync_snapshot().unwrap();
        let restored = StateSnapshot::from_sync_snapshot(&sync).unwrap();

        // Nonces must survive the round-trip
        let restored_nonces: HashMap<[u8; 32], u64> =
            postcard::from_bytes(&restored.nonce_state_bytes).unwrap();
        assert_eq!(restored_nonces.len(), 3);
        assert_eq!(restored_nonces[&test_node(1)], 100);
        assert_eq!(restored_nonces[&test_node(2)], 200);
        assert_eq!(restored_nonces[&test_node(3)], 300);
    }
}
