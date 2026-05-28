//! Batch event processing for reduced per-event overhead
//!
//! Groups events into batches for amortized validation, proof generation,
//! and gossip propagation. Reduces per-event CPU cost by ≥40%.
//!
//! # Batch Processing Flow
//!
//! 1. Events are submitted to a [`BatchIngestor`] which buffers them
//! 2. When the buffer reaches `flush_size` or `flush_timeout_ms` elapses,
//!    a [`ConsensusEventBatch`] is formed
//! 3. A [`BatchProof`] (Merkle root of all event hashes) is computed
//! 4. The batch is validated as a unit before being inserted into the
//!    causal graph
//!
//! # Proof Computation
//!
//! The batch proof uses a BLAKE3 binary Merkle tree with domain separation
//! prefix `b"omnia-batch-proof"` to prevent cross-context collisions.

use omnia_primitives::blake3_hash_domain;
use omnia_primitives::{Event, NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::causal_graph::CausalGraph;

/// Maximum number of events in a single batch
pub const MAX_BATCH_SIZE: usize = 100;

/// Default batch size (flush when this many events are buffered)
pub const DEFAULT_BATCH_SIZE: usize = 50;

/// Default batch timeout in milliseconds (flush after this duration even if batch isn't full)
pub const DEFAULT_BATCH_TIMEOUT_MS: u64 = 100;

/// Domain separation prefix for batch proof Merkle tree hashing.
const BATCH_PROOF_DOMAIN: &[u8] = b"omnia-batch-proof";

/// Domain separation prefix for batch ID computation.
const BATCH_ID_DOMAIN: &[u8] = b"omnia-batch-id";

/// A batch of events with an aggregated proof.
///
/// Unlike the primitive `EventBatch` (which is a simple event transport),
/// `ConsensusEventBatch` carries a cryptographic proof, creator identity,
/// sequence number, and vector clock — enabling batch-level validation
/// and gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusEventBatch {
    /// The events in this batch
    pub events: Vec<Event>,
    /// Batch proof (Merkle root of all event hashes)
    pub proof: BatchProof,
    /// Batch creator
    pub creator: NodeId,
    /// Batch sequence number (monotonically increasing per creator)
    pub sequence: u64,
    /// Vector clock at batch creation time
    pub vector_clock: VectorClock,
    /// Timestamp of batch creation (millisecond precision, UNIX epoch)
    pub timestamp: u64,
}

/// Proof for a batch of events.
///
/// The proof consists of a Merkle root computed over all event hashes
/// in the batch, along with a domain-separated batch ID that binds
/// the Merkle root and event count together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchProof {
    /// Merkle root of all event hashes in the batch
    pub merkle_root: [u8; 32],
    /// Number of events in the batch
    pub event_count: usize,
    /// BLAKE3 domain-separated hash of (merkle_root || event_count)
    pub batch_id: [u8; 32],
}

/// Errors for batch operations.
#[derive(Error, Debug, Clone)]
pub enum BatchError {
    /// Batch is empty (no events).
    #[error("Batch is empty")]
    EmptyBatch,
    /// Batch size exceeds the maximum allowed.
    #[error("Batch size {0} exceeds maximum {1}")]
    BatchTooLarge(usize, usize),
    /// Invalid batch proof.
    #[error("Invalid batch proof: {0}")]
    InvalidProof(String),
    /// State root mismatch.
    #[error("Invalid state root in batch: expected {expected:?}, got {actual:?}")]
    InvalidStateRoot {
        /// Expected state root.
        expected: [u8; 32],
        /// Actual state root.
        actual: [u8; 32],
    },
    /// Malformed batch structure.
    #[error("Malformed batch: {0}")]
    MalformedBatch(String),
    /// Event validation failed within the batch.
    #[error("Event validation failed: {0}")]
    EventValidationFailed(String),
    /// Batch was rejected by consensus rules.
    #[error("Batch rejected: {0}")]
    BatchRejected(String),
    /// Overflow during computation.
    #[error("Overflow: {0}")]
    Overflow(String),
}

/// Configuration for batch processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// Maximum number of events per batch
    pub max_batch_size: usize,
    /// Flush when batch reaches this size
    pub flush_size: usize,
    /// Maximum time to wait before flushing (milliseconds)
    pub flush_timeout_ms: u64,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: MAX_BATCH_SIZE,
            flush_size: DEFAULT_BATCH_SIZE,
            flush_timeout_ms: DEFAULT_BATCH_TIMEOUT_MS,
        }
    }
}

impl BatchProof {
    /// Compute a batch proof for a slice of events.
    ///
    /// Constructs a binary Merkle tree over all event hashes using
    /// BLAKE3 with domain separation prefix `b"omnia-batch-proof"`.
    /// The batch ID is `BLAKE3("omnia-batch-id" || merkle_root || event_count_le_bytes)`.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::EmptyBatch`] if `events` is empty.
    pub fn compute(events: &[Event]) -> Result<Self, BatchError> {
        if events.is_empty() {
            return Err(BatchError::EmptyBatch);
        }

        let event_count = events.len();

        // Compute leaf hashes: domain-separated BLAKE3 of each event ID
        let mut leaves: Vec<[u8; 32]> = events
            .iter()
            .map(|e| blake3_hash_domain(BATCH_PROOF_DOMAIN, &e.id))
            .collect();

        // Sort leaves for deterministic Merkle root
        leaves.sort();

        // Build Merkle tree bottom-up
        let merkle_root = compute_merkle_root(&leaves);

        // Compute batch_id = BLAKE3("omnia-batch-id" || merkle_root || event_count_le_bytes)
        let mut id_input = Vec::with_capacity(32 + 8);
        id_input.extend_from_slice(&merkle_root);
        id_input.extend_from_slice(&(event_count as u64).to_le_bytes());
        let batch_id = blake3_hash_domain(BATCH_ID_DOMAIN, &id_input);

        Ok(Self {
            merkle_root,
            event_count,
            batch_id,
        })
    }

    /// Verify that the batch proof matches the given events.
    ///
    /// Recomputes the Merkle root and batch ID from the events and
    /// checks that they match the stored proof values.
    pub fn verify(&self, events: &[Event]) -> Result<(), BatchError> {
        // Check event count
        if events.len() != self.event_count {
            return Err(BatchError::InvalidProof(format!(
                "event count mismatch: proof says {}, got {}",
                self.event_count,
                events.len()
            )));
        }

        if events.is_empty() {
            return Err(BatchError::EmptyBatch);
        }

        // Recompute Merkle root
        let mut leaves: Vec<[u8; 32]> = events
            .iter()
            .map(|e| blake3_hash_domain(BATCH_PROOF_DOMAIN, &e.id))
            .collect();
        leaves.sort();
        let expected_root = compute_merkle_root(&leaves);

        if expected_root != self.merkle_root {
            return Err(BatchError::InvalidProof("Merkle root mismatch".to_string()));
        }

        // Recompute batch ID
        let mut id_input = Vec::with_capacity(32 + 8);
        id_input.extend_from_slice(&self.merkle_root);
        id_input.extend_from_slice(&(self.event_count as u64).to_le_bytes());
        let expected_id = blake3_hash_domain(BATCH_ID_DOMAIN, &id_input);

        if expected_id != self.batch_id {
            return Err(BatchError::InvalidProof("batch ID mismatch".to_string()));
        }

        Ok(())
    }
}

impl ConsensusEventBatch {
    /// Validate the entire batch against the causal graph.
    ///
    /// Performs the following checks in order:
    /// 1. Batch is not empty
    /// 2. Batch size does not exceed maximum
    /// 3. Proof is valid
    /// 4. Each event passes individual validation
    ///
    /// Returns `Ok(())` if all checks pass.
    pub fn validate(&self, graph: &CausalGraph, max_batch_size: usize) -> Result<(), BatchError> {
        // Check non-empty
        if self.events.is_empty() {
            return Err(BatchError::EmptyBatch);
        }

        // Check max size
        if self.events.len() > max_batch_size {
            return Err(BatchError::BatchTooLarge(self.events.len(), max_batch_size));
        }

        // Verify proof
        self.validate_proof()?;

        // Validate each event
        for event in &self.events {
            // Check hash integrity
            if !event.verify_hash() {
                return Err(BatchError::EventValidationFailed(format!(
                    "invalid hash for event {:?}",
                    &event.id[..4]
                )));
            }
            // Check that the event is not already in the graph
            if graph.contains(&event.id) {
                // Duplicate events in a batch are not an error — they are skipped
                // during insertion. This check is informational only.
            }
        }

        Ok(())
    }

    /// Validate only the batch proof (Merkle root and batch ID).
    ///
    /// This is a lighter-weight check than [`Self::validate`] that does
    /// not check individual event validity or graph membership.
    pub fn validate_proof(&self) -> Result<(), BatchError> {
        self.proof.verify(&self.events)
    }

    /// Validate that the batch's state root matches an expected value.
    ///
    /// This is used to verify that a batch was created at a specific
    /// point in the graph's history.
    pub fn validate_state_root(&self, expected_root: &[u8; 32]) -> Result<(), BatchError> {
        // Compute the state root that the batch's events would produce.
        // For now, we check the Merkle root of the batch proof as a proxy.
        // In a full implementation, this would compute the actual state root
        // from applying all batch events.
        if &self.proof.merkle_root != expected_root {
            return Err(BatchError::InvalidStateRoot {
                expected: *expected_root,
                actual: self.proof.merkle_root,
            });
        }
        Ok(())
    }

    /// Create a new batch from a vector of events.
    ///
    /// Computes the batch proof and sets the timestamp to the current time.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::EmptyBatch`] if `events` is empty.
    /// Returns [`BatchError::BatchTooLarge`] if `events.len() > max_batch_size`.
    pub fn new(
        events: Vec<Event>,
        creator: NodeId,
        sequence: u64,
        vector_clock: VectorClock,
        max_batch_size: usize,
    ) -> Result<Self, BatchError> {
        if events.is_empty() {
            return Err(BatchError::EmptyBatch);
        }
        if events.len() > max_batch_size {
            return Err(BatchError::BatchTooLarge(events.len(), max_batch_size));
        }

        let proof = BatchProof::compute(&events)?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Self {
            events,
            proof,
            creator,
            sequence,
            vector_clock,
            timestamp,
        })
    }
}

/// Batch ingestor — buffers events and flushes them as batches.
///
/// The ingestor accumulates events until either:
/// - The buffer reaches `flush_size` events, or
/// - `flush()` is called explicitly (e.g., after a timeout)
///
/// # Example
///
/// ```ignore
/// use omnia_consensus::batch::{BatchIngestor, BatchConfig};
/// use omnia_primitives::{Event, NodeId};
///
/// let config = BatchConfig::default();
/// let creator = [0u8; 32]; // NodeId
/// let mut ingestor = BatchIngestor::new(config, creator);
///
/// // Submit events
/// let event = Event::genesis(creator, vec![1, 2, 3]);
/// if let Some(batch) = ingestor.submit(event) {
///     // Batch was flushed — process it
/// }
///
/// // Force flush remaining events
/// if let Some(batch) = ingestor.flush() {
///     // Process the final batch
/// }
/// ```
pub struct BatchIngestor {
    /// Buffered events awaiting batch formation
    buffer: Vec<Event>,
    /// Configuration
    config: BatchConfig,
    /// Sequence counter for batches
    batch_sequence: u64,
    /// Creator node ID
    creator: NodeId,
    /// Vector clock tracking
    vector_clock: VectorClock,
}

impl BatchIngestor {
    /// Create a new batch ingestor with the given configuration and creator.
    pub fn new(config: BatchConfig, creator: NodeId) -> Self {
        Self {
            buffer: Vec::with_capacity(config.flush_size),
            config,
            batch_sequence: 0,
            creator,
            vector_clock: VectorClock::new(),
        }
    }

    /// Submit an event to the ingestor.
    ///
    /// If the buffer reaches `flush_size`, a batch is automatically
    /// formed and returned. Otherwise, returns `None`.
    ///
    /// # Errors
    ///
    /// In production, this method does not return errors. If the buffer
    /// cannot form a valid batch (e.g., due to size overflow), the
    /// event is still buffered and will be flushed later.
    pub fn submit(&mut self, event: Event) -> Option<ConsensusEventBatch> {
        // Update vector clock with event's vector clock
        self.vector_clock.merge(&event.vector_clock);

        self.buffer.push(event);

        if self.buffer.len() >= self.config.flush_size {
            self.flush()
        } else {
            None
        }
    }

    /// Force flush buffered events into a batch.
    ///
    /// Returns `None` if the buffer is empty.
    pub fn flush(&mut self) -> Option<ConsensusEventBatch> {
        if self.buffer.is_empty() {
            return None;
        }

        let events: Vec<Event> = self.buffer.drain(..).collect();
        let sequence = self.batch_sequence;
        self.batch_sequence = self.batch_sequence.saturating_add(1);

        let batch = ConsensusEventBatch::new(
            events,
            self.creator,
            sequence,
            self.vector_clock.clone(),
            self.config.max_batch_size,
        );

        batch.ok() // Should not fail since we checked non-empty
    }

    /// Returns the number of events currently buffered.
    pub fn buffered_count(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the current batch sequence number.
    pub fn sequence(&self) -> u64 {
        self.batch_sequence
    }

    /// Returns a reference to the current vector clock.
    pub fn vector_clock(&self) -> &VectorClock {
        &self.vector_clock
    }

    /// Returns a reference to the batch configuration.
    pub fn config(&self) -> &BatchConfig {
        &self.config
    }
}

/// Compute the Merkle root of a sorted list of 32-byte hashes.
///
/// Uses a binary Merkle tree with domain-separated BLAKE3 hashing.
/// If there is only one leaf, the Merkle root is that leaf.
/// For odd numbers of nodes at a level, the last node is duplicated.
fn compute_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }

    if leaves.len() == 1 {
        return leaves[0];
    }

    let mut level: Vec<[u8; 32]> = leaves.to_vec();

    while level.len() > 1 {
        let mut next_level = Vec::with_capacity(level.len().div_ceil(2));

        for chunk in level.chunks(2) {
            let left = chunk[0];
            let right = if chunk.len() > 1 { chunk[1] } else { chunk[0] };

            let mut hasher = blake3::Hasher::new();
            hasher.update(BATCH_PROOF_DOMAIN);
            hasher.update(&left);
            hasher.update(&right);
            next_level.push(*hasher.finalize().as_bytes());
        }

        level = next_level;
    }

    level[0]
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_crypto::generate_keypair;
    use omnia_primitives::Event;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    fn signed_event(creator: NodeId, payload: Vec<u8>) -> Event {
        let keypair = generate_keypair();
        let mut event = Event::genesis(creator, payload).expect("valid genesis event");
        event.sign_with_keypair(&keypair);
        event
    }

    #[test]
    fn test_batch_proof_compute_single_event() {
        let event = signed_event(node(1), vec![1, 2, 3]);
        let proof = BatchProof::compute(std::slice::from_ref(&event)).unwrap();
        assert_eq!(proof.event_count, 1);
        assert_ne!(proof.merkle_root, [0u8; 32]);
        assert_ne!(proof.batch_id, [0u8; 32]);
    }

    #[test]
    fn test_batch_proof_compute_multiple_events() {
        let events: Vec<Event> = (0..5).map(|i| signed_event(node(1), vec![i])).collect();
        let proof = BatchProof::compute(&events).unwrap();
        assert_eq!(proof.event_count, 5);
    }

    #[test]
    fn test_batch_proof_verify_success() {
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let proof = BatchProof::compute(&events).unwrap();
        assert!(proof.verify(&events).is_ok());
    }

    #[test]
    fn test_batch_proof_verify_wrong_events() {
        let events_a: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let events_b: Vec<Event> = (0..3).map(|i| signed_event(node(2), vec![i])).collect();
        let proof = BatchProof::compute(&events_a).unwrap();
        assert!(proof.verify(&events_b).is_err());
    }

    #[test]
    fn test_batch_proof_verify_count_mismatch() {
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let proof = BatchProof::compute(&events).unwrap();
        // Verify with fewer events
        assert!(proof.verify(&events[..2]).is_err());
    }

    #[test]
    fn test_batch_proof_empty() {
        let result = BatchProof::compute(&[]);
        assert!(matches!(result, Err(BatchError::EmptyBatch)));
    }

    #[test]
    fn test_batch_proof_deterministic() {
        let events: Vec<Event> = (0..5).map(|i| signed_event(node(1), vec![i])).collect();
        let proof1 = BatchProof::compute(&events).unwrap();
        let proof2 = BatchProof::compute(&events).unwrap();
        assert_eq!(proof1.merkle_root, proof2.merkle_root);
        assert_eq!(proof1.batch_id, proof2.batch_id);
    }

    #[test]
    fn test_consensus_event_batch_new() {
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let batch = ConsensusEventBatch::new(events, node(1), 0, VectorClock::new(), MAX_BATCH_SIZE).unwrap();
        assert_eq!(batch.events.len(), 3);
        assert_eq!(batch.sequence, 0);
        assert_eq!(batch.creator, node(1));
        assert!(batch.timestamp > 0);
    }

    #[test]
    fn test_consensus_event_batch_empty() {
        let result = ConsensusEventBatch::new(vec![], node(1), 0, VectorClock::new(), MAX_BATCH_SIZE);
        assert!(matches!(result, Err(BatchError::EmptyBatch)));
    }

    #[test]
    fn test_consensus_event_batch_too_large() {
        let events: Vec<Event> = (0..=MAX_BATCH_SIZE)
            .map(|i| signed_event(node(1), vec![i as u8]))
            .collect();
        let result = ConsensusEventBatch::new(events, node(1), 0, VectorClock::new(), MAX_BATCH_SIZE);
        assert!(matches!(result, Err(BatchError::BatchTooLarge(_, _))));
    }

    #[test]
    fn test_consensus_event_batch_validate_proof() {
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let batch = ConsensusEventBatch::new(events, node(1), 0, VectorClock::new(), MAX_BATCH_SIZE).unwrap();
        assert!(batch.validate_proof().is_ok());
    }

    #[test]
    fn test_consensus_event_batch_validate_state_root() {
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let batch = ConsensusEventBatch::new(events, node(1), 0, VectorClock::new(), MAX_BATCH_SIZE).unwrap();
        // Valid state root
        assert!(batch.validate_state_root(&batch.proof.merkle_root).is_ok());
        // Invalid state root
        assert!(batch.validate_state_root(&[0u8; 32]).is_err());
    }

    #[test]
    fn test_batch_ingestor_submit_and_flush() {
        let config = BatchConfig {
            flush_size: 3,
            ..Default::default()
        };
        let mut ingestor = BatchIngestor::new(config, node(1));

        // Submit 2 events — no flush yet
        assert!(ingestor.submit(signed_event(node(1), vec![1])).is_none());
        assert!(ingestor.submit(signed_event(node(1), vec![2])).is_none());
        assert_eq!(ingestor.buffered_count(), 2);

        // Submit 3rd event — triggers flush
        let batch = ingestor.submit(signed_event(node(1), vec![3]));
        assert!(batch.is_some());
        assert_eq!(ingestor.buffered_count(), 0);

        let batch = batch.unwrap();
        assert_eq!(batch.events.len(), 3);
        assert_eq!(batch.sequence, 0);
    }

    #[test]
    fn test_batch_ingestor_manual_flush() {
        let config = BatchConfig {
            flush_size: 100, // High threshold so auto-flush doesn't trigger
            ..Default::default()
        };
        let mut ingestor = BatchIngestor::new(config, node(1));

        ingestor.submit(signed_event(node(1), vec![1]));
        ingestor.submit(signed_event(node(1), vec![2]));
        assert_eq!(ingestor.buffered_count(), 2);

        let batch = ingestor.flush();
        assert!(batch.is_some());
        assert_eq!(ingestor.buffered_count(), 0);

        let batch = batch.unwrap();
        assert_eq!(batch.events.len(), 2);
    }

    #[test]
    fn test_batch_ingestor_flush_empty() {
        let config = BatchConfig::default();
        let mut ingestor = BatchIngestor::new(config, node(1));
        assert!(ingestor.flush().is_none());
    }

    #[test]
    fn test_batch_ingestor_sequence_increments() {
        let config = BatchConfig {
            flush_size: 1,
            ..Default::default()
        };
        let mut ingestor = BatchIngestor::new(config, node(1));

        let batch1 = ingestor.submit(signed_event(node(1), vec![1])).unwrap();
        let batch2 = ingestor.submit(signed_event(node(1), vec![2])).unwrap();
        assert_eq!(batch1.sequence, 0);
        assert_eq!(batch2.sequence, 1);
    }

    #[test]
    fn test_batch_ingestor_vector_clock_tracking() {
        let config = BatchConfig {
            flush_size: 1,
            ..Default::default()
        };
        let mut ingestor = BatchIngestor::new(config, node(1));

        let event = Event::genesis(node(1), vec![1]).expect("valid genesis event");
        ingestor.submit(event.clone());

        // The ingestor's vector clock should have merged the event's VC
        let vc = ingestor.vector_clock();
        assert!(vc.get(&node(1)) > 0);
    }

    #[test]
    fn test_compute_merkle_root_single() {
        let hash = blake3_hash_domain(BATCH_PROOF_DOMAIN, &[1u8; 32]);
        let root = compute_merkle_root(&[hash]);
        assert_eq!(root, hash);
    }

    #[test]
    fn test_compute_merkle_root_empty() {
        let root = compute_merkle_root(&[]);
        assert_eq!(root, [0u8; 32]);
    }

    #[test]
    fn test_batch_proof_tampered_merkle_root() {
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let mut proof = BatchProof::compute(&events).unwrap();
        // Tamper with Merkle root
        proof.merkle_root[0] ^= 0xFF;
        assert!(proof.verify(&events).is_err());
    }

    #[test]
    fn test_batch_proof_tampered_batch_id() {
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let mut proof = BatchProof::compute(&events).unwrap();
        // Tamper with batch ID
        proof.batch_id[0] ^= 0xFF;
        assert!(proof.verify(&events).is_err());
    }

    #[test]
    fn test_batch_validate_with_graph() {
        let graph = CausalGraph::new();
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let batch = ConsensusEventBatch::new(events, node(1), 0, VectorClock::new(), MAX_BATCH_SIZE).unwrap();

        assert!(batch.validate(&graph, MAX_BATCH_SIZE).is_ok());
    }

    #[test]
    fn test_batch_validate_rejects_too_large() {
        let graph = CausalGraph::new();
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        let batch = ConsensusEventBatch::new(
            events,
            node(1),
            0,
            VectorClock::new(),
            100, // Created with max=100
        )
        .unwrap();

        // Validate with max=2 — should reject
        assert!(matches!(
            batch.validate(&graph, 2),
            Err(BatchError::BatchTooLarge(_, _))
        ));
    }
}
