//! Supply Chain Provenance Log — Append-Only CRDT
//!
//! Physical items move through supply chains. Each movement is an event in
//! the causal graph. The provenance is an append-only log — a CRDT that
//! never deletes, only grows. This is the most important component of the
//! Binding Layer because it provides the immutable chain of custody that
//! replaces trusted third-party attestations.
//!
//! # CRDT Properties
//!
//! The provenance log is an append-only CRDT (CvRDT). Appends are:
//! - **Commutative**: The order of appending events from different shards
//!   doesn't matter — they all end up in the log.
//! - **Associative**: Grouping of appends doesn't affect the result.
//! - **Idempotent**: Appending the same event twice has no effect.
//!
//! # Chain Integrity
//!
//! Each `ProvenanceEvent` contains a `QuantumCommitment` that
//! cryptographically links to the previous event. This creates a
//! hash chain (similar to a blockchain) but anchored in the causal graph
//! rather than a linear chain. Breaking any link invalidates the entire
//! chain from that point forward.

use omnia_substrate::{EventId, VectorClock};
use serde::{Deserialize, Serialize};

use crate::quantum_commit::QuantumCommitment;
use crate::rf_fingerprint::RfFingerprint;

/// The type of provenance event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProvenanceEventType {
    /// Item was created / first anchored on-chain.
    Created,
    /// Ownership was transferred from one holder to another.
    Transferred,
    /// Item was verified (RF check + commitment verification).
    Verified,
    /// Item was destroyed / decommissioned.
    Destroyed,
}

impl std::fmt::Display for ProvenanceEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvenanceEventType::Created => write!(f, "Created"),
            ProvenanceEventType::Transferred => write!(f, "Transferred"),
            ProvenanceEventType::Verified => write!(f, "Verified"),
            ProvenanceEventType::Destroyed => write!(f, "Destroyed"),
        }
    }
}

/// Sentinel value for `previous_hash` on the genesis (first) event.
const GENESIS_PREV: [u8; 32] = [0u8; 32];

/// A single event in an item's provenance chain.
///
/// Each event records who transferred to whom, with cryptographic proof
/// (RF fingerprint + quantum commitment) and a causal timestamp.
/// Two hash-chain fields provide tamper-evidence:
///
/// - **`previous_hash`** — the `entry_hash` of the preceding event
///   (`[0u8; 32]` for the genesis event).
/// - **`entry_hash`** — `BLAKE3(previous_hash || commitment.data_hash)`,
///   committing this event to the chain.
///
/// An attacker who reorders, inserts, or modifies any event will break
/// the chain because every `entry_hash` transitively depends on all
/// preceding events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceEvent {
    /// The type of event: Created, Transferred, Verified, or Destroyed.
    pub event_type: ProvenanceEventType,
    /// Previous holder (None for creation events).
    pub from: Option<String>,
    /// New holder (DID string).
    pub to: String,
    /// RF fingerprint of the item at this event.
    pub rf_proof: RfFingerprint,
    /// Quantum-resistant commitment of event data.
    pub commitment: QuantumCommitment,
    /// `entry_hash` of the preceding `ProvenanceEvent`.
    /// `[0u8; 32]` for the genesis (first) event.
    /// Backward-compatible: defaults to all-zeros when deserializing old data.
    #[serde(default = "default_hash")]
    pub previous_hash: [u8; 32],
    /// `BLAKE3(previous_hash || commitment.data_hash)` — the hash that
    /// the *next* event must reference in its `previous_hash` field.
    /// Backward-compatible: defaults to all-zeros when deserializing old data.
    #[serde(default = "default_hash")]
    pub entry_hash: [u8; 32],
    /// Causal timestamp (vector clock).
    pub timestamp: VectorClock,
}

/// Default value for `previous_hash` / `entry_hash` — all zeros.
const fn default_hash() -> [u8; 32] {
    [0u8; 32]
}

impl ProvenanceEvent {
    /// Compute `entry_hash = BLAKE3(previous_hash || data_hash)`.
    fn compute_entry_hash(previous_hash: &[u8; 32], data_hash: &[u8; 32]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(previous_hash);
        hasher.update(data_hash);
        *hasher.finalize().as_bytes()
    }
}

/// Errors that can occur during provenance log operations.
#[derive(Debug, thiserror::Error)]
pub enum ProvenanceError {
    /// The provenance log has no events (invalid state).
    #[error("provenance log has no events")]
    EmptyLog,
    /// Deserialization failed.
    #[error("deserialization error: {0}")]
    DeserializationError(String),
    /// Serialization failed.
    #[error("serialization error: {0}")]
    SerializationError(String),
    /// Invalid log structure.
    #[error("invalid log: {0}")]
    InvalidLog(String),
}

/// An append-only provenance log for a single physical item.
///
/// The log records the complete chain of custody for an item, from
/// creation through any number of transfers, verifications, and
/// eventual destruction. It is append-only: no events can be deleted
/// or modified once added.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceLog {
    /// Item being tracked (linked to PhysicalShard).
    pub item_id: [u8; 32],
    /// Append-only sequence of custody events.
    pub events: Vec<ProvenanceEvent>,
    /// Current holder (DID string).
    pub current_holder: String,
    /// Causal graph anchor (EventId of the latest provenance event).
    pub latest_anchor: EventId,
}

impl ProvenanceLog {
    /// Version byte prefixed to serialized snapshots for format migration.
    const PROVENANCE_LOG_VERSION: u8 = 1;

    /// Create a new provenance log for an item.
    ///
    /// The genesis event's `previous_hash` is `[0u8; 32]` and its
    /// `entry_hash` is `BLAKE3([0u8; 32] || commitment.data_hash)`.
    /// The commitment's `previous_hash` is also set to `[0u8; 32]`
    /// (genesis / legacy default).
    ///
    /// # Arguments
    ///
    /// * `item_id` — 32-byte unique identifier for the item
    /// * `creator` — DID of the initial holder
    /// * `rf_proof` — RF fingerprint at creation time
    /// * `commitment` — Quantum commitment of the creation event
    /// * `anchor` — EventId in the causal graph for this creation event
    pub fn new(
        item_id: [u8; 32],
        creator: String,
        rf_proof: RfFingerprint,
        commitment: QuantumCommitment,
        anchor: EventId,
    ) -> Self {
        let previous_hash = GENESIS_PREV;
        let entry_hash = ProvenanceEvent::compute_entry_hash(&previous_hash, &commitment.data_hash);

        let creation_event = ProvenanceEvent {
            event_type: ProvenanceEventType::Created,
            from: None,
            to: creator.clone(),
            rf_proof,
            commitment,
            previous_hash,
            entry_hash,
            timestamp: VectorClock::new(),
        };

        Self {
            item_id,
            events: vec![creation_event],
            current_holder: creator,
            latest_anchor: anchor,
        }
    }

    /// Transfer ownership of the item to a new holder.
    ///
    /// Creates a new `ProvenanceEvent` of type `Transferred`, appends it
    /// to the log, and updates the current holder. The new event's
    /// `previous_hash` is set to the previous event's `entry_hash`, and
    /// its `entry_hash` is computed as
    /// `BLAKE3(previous_entry_hash || commitment.data_hash)`. The
    /// commitment's `previous_hash` is set to
    /// `BLAKE3(prev_commitment.data_hash)` to maintain the commitment-level
    /// chain.
    ///
    /// # Arguments
    ///
    /// * `to` — DID of the new holder
    /// * `rf_proof` — RF fingerprint at transfer time
    /// * `commitment` — Quantum commitment of the transfer event
    ///
    /// # Returns
    ///
    /// The newly created `ProvenanceEvent`.
    pub fn transfer(
        &mut self,
        to: String,
        rf_proof: RfFingerprint,
        mut commitment: QuantumCommitment,
    ) -> ProvenanceEvent {
        let prev = self
            .events
            .last()
            .expect("provenance log always has at least one event");

        // Set the commitment's previous_hash to link to the previous commitment
        commitment.previous_hash = QuantumCommitment::compute_chain_hash(&prev.commitment.data_hash);

        let previous_hash = prev.entry_hash;
        let entry_hash = ProvenanceEvent::compute_entry_hash(&previous_hash, &commitment.data_hash);

        let event = ProvenanceEvent {
            event_type: ProvenanceEventType::Transferred,
            from: Some(self.current_holder.clone()),
            to: to.clone(),
            rf_proof,
            commitment,
            previous_hash,
            entry_hash,
            timestamp: VectorClock::new(),
        };
        self.events.push(event.clone());
        self.current_holder = to;
        event
    }

    /// Record a verification event for the item.
    ///
    /// Verification checks that the item's current RF signature matches
    /// its registered fingerprint and that all commitments are valid.
    /// The new event is cryptographically linked to the previous event
    /// via `previous_hash` / `entry_hash` and the commitment chain.
    pub fn verify(&mut self, rf_proof: RfFingerprint, mut commitment: QuantumCommitment) -> ProvenanceEvent {
        let prev = self
            .events
            .last()
            .expect("provenance log always has at least one event");

        commitment.previous_hash = QuantumCommitment::compute_chain_hash(&prev.commitment.data_hash);

        let previous_hash = prev.entry_hash;
        let entry_hash = ProvenanceEvent::compute_entry_hash(&previous_hash, &commitment.data_hash);

        let event = ProvenanceEvent {
            event_type: ProvenanceEventType::Verified,
            from: None,
            to: self.current_holder.clone(),
            rf_proof,
            commitment,
            previous_hash,
            entry_hash,
            timestamp: VectorClock::new(),
        };
        self.events.push(event.clone());
        event
    }

    /// Record a destruction event for the item.
    ///
    /// Once an item is destroyed, no further transfers are possible.
    /// The new event is cryptographically linked to the previous event
    /// via `previous_hash` / `entry_hash` and the commitment chain.
    pub fn destroy(&mut self, rf_proof: RfFingerprint, mut commitment: QuantumCommitment) -> ProvenanceEvent {
        let prev = self
            .events
            .last()
            .expect("provenance log always has at least one event");

        commitment.previous_hash = QuantumCommitment::compute_chain_hash(&prev.commitment.data_hash);

        let previous_hash = prev.entry_hash;
        let entry_hash = ProvenanceEvent::compute_entry_hash(&previous_hash, &commitment.data_hash);

        let event = ProvenanceEvent {
            event_type: ProvenanceEventType::Destroyed,
            from: Some(self.current_holder.clone()),
            to: String::new(), // No new holder
            rf_proof,
            commitment,
            previous_hash,
            entry_hash,
            timestamp: VectorClock::new(),
        };
        self.events.push(event.clone());
        event
    }

    /// Verify the integrity of the entire provenance chain.
    ///
    /// Checks that every consecutive pair of events has a valid
    /// cryptographic link along **two** independent chains:
    ///
    /// 1. **Commitment chain**: `curr.commitment.previous_hash ==
    ///    BLAKE3(prev.commitment.data_hash)` (checked via `links_to`).
    /// 2. **Event chain**: `curr.previous_hash == prev.entry_hash` AND
    ///    `prev.entry_hash == BLAKE3(prev.previous_hash ||
    ///    prev.commitment.data_hash)`.
    ///
    /// The first (genesis) event must have `previous_hash == [0u8; 32]`.
    ///
    /// # Returns
    ///
    /// `true` if the chain is intact, `false` if any link is broken.
    pub fn verify_chain(&self) -> bool {
        if self.events.is_empty() {
            return true;
        }

        // First event must be a Created event with genesis previous_hash
        let first = &self.events[0];
        if first.event_type != ProvenanceEventType::Created {
            return false;
        }
        if first.previous_hash != GENESIS_PREV {
            return false;
        }

        // Verify genesis entry_hash
        let expected_entry = ProvenanceEvent::compute_entry_hash(&GENESIS_PREV, &first.commitment.data_hash);
        if first.entry_hash != expected_entry {
            return false;
        }

        // Walk the chain, verifying each pair
        for window in self.events.windows(2) {
            let prev = &window[0];
            let curr = &window[1];

            // 1. Commitment-level chain
            if !curr.commitment.links_to(&prev.commitment) {
                return false;
            }

            // 2. Event-level chain: previous_hash must equal prev.entry_hash
            if curr.previous_hash != prev.entry_hash {
                return false;
            }

            // 3. Recompute curr.entry_hash and verify it matches
            let expected = ProvenanceEvent::compute_entry_hash(&curr.previous_hash, &curr.commitment.data_hash);
            if curr.entry_hash != expected {
                return false;
            }
        }

        true
    }

    /// Get the number of events in the provenance log.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if the provenance log is empty (should never be true after
    /// construction, since `new()` always adds a Created event).
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get the event at a specific index.
    pub fn get_event(&self, index: usize) -> Option<&ProvenanceEvent> {
        self.events.get(index)
    }

    /// Get the creation event (first event in the log).
    ///
    /// Returns an error if the log is empty (should never happen after
    /// construction via `new()`, but can occur after deserialization of
    /// corrupt data).
    pub fn creation_event(&self) -> Result<&ProvenanceEvent, ProvenanceError> {
        self.events.first().ok_or(ProvenanceError::EmptyLog)
    }

    /// Check if the item has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.events
            .last()
            .map(|e| e.event_type == ProvenanceEventType::Destroyed)
            .unwrap_or(false)
    }

    /// Serialize the provenance log to bytes.
    ///
    /// The output is prefixed with a version byte to support future
    /// state-format migrations.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProvenanceError> {
        let mut bytes = vec![Self::PROVENANCE_LOG_VERSION];
        let serialized = postcard::to_allocvec(self)
            .map_err(|e| ProvenanceError::SerializationError(e.to_string()))?;
        bytes.extend(serialized);
        Ok(bytes)
    }

    /// Deserialize a provenance log from bytes.
    ///
    /// Reads and validates the version byte before deserializing the
    /// payload. Returns an error if the version is unsupported.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProvenanceError> {
        if bytes.is_empty() {
            return Err(ProvenanceError::DeserializationError("empty input".into()));
        }
        let version = bytes[0];
        if version != Self::PROVENANCE_LOG_VERSION {
            return Err(ProvenanceError::DeserializationError(format!(
                "unsupported version: {}",
                version
            )));
        }
        let log: Self = postcard::from_bytes(&bytes[1..])
            .map_err(|e| ProvenanceError::DeserializationError(e.to_string()))?;
        if log.events.is_empty() {
            return Err(ProvenanceError::InvalidLog(
                "provenance log must have at least one event".into(),
            ));
        }
        Ok(log)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::quantum_commit::QuantumCommitment;
    use crate::rf_fingerprint::RfFingerprint;

    fn test_rf(did: &str) -> RfFingerprint {
        RfFingerprint::stub(did, [0x55u8; 32])
    }

    fn test_commitment(data: &[u8]) -> QuantumCommitment {
        QuantumCommitment::new_stub(data, VectorClock::new())
    }

    fn test_item_id() -> [u8; 32] {
        [0xABu8; 32]
    }

    fn test_anchor() -> EventId {
        [0xCDu8; 32]
    }

    #[test]
    fn test_create_provenance_log() {
        let log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:creator".to_string(),
            test_rf("did:omnia:creator"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        assert_eq!(log.len(), 1);
        assert_eq!(log.current_holder, "did:omnia:creator");
        assert_eq!(log.events[0].event_type, ProvenanceEventType::Created);
        assert!(log.events[0].from.is_none());
        assert_eq!(log.events[0].to, "did:omnia:creator");
    }

    #[test]
    fn test_transfer_ownership() {
        let mut log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:alice".to_string(),
            test_rf("did:omnia:alice"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        log.transfer(
            "did:omnia:bob".to_string(),
            test_rf("did:omnia:bob"),
            test_commitment(b"transfer1"),
        );

        assert_eq!(log.len(), 2);
        assert_eq!(log.current_holder, "did:omnia:bob");
        assert_eq!(log.events[1].event_type, ProvenanceEventType::Transferred);
        assert_eq!(log.events[1].from, Some("did:omnia:alice".to_string()));
        assert_eq!(log.events[1].to, "did:omnia:bob");
    }

    #[test]
    fn test_verify_event() {
        let mut log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:alice".to_string(),
            test_rf("did:omnia:alice"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        log.verify(test_rf("did:omnia:alice"), test_commitment(b"verification"));

        assert_eq!(log.len(), 2);
        assert_eq!(log.events[1].event_type, ProvenanceEventType::Verified);
        assert_eq!(log.events[1].to, "did:omnia:alice");
    }

    #[test]
    fn test_destroy_event() {
        let mut log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:alice".to_string(),
            test_rf("did:omnia:alice"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        log.destroy(test_rf("did:omnia:alice"), test_commitment(b"destruction"));

        assert_eq!(log.len(), 2);
        assert_eq!(log.events[1].event_type, ProvenanceEventType::Destroyed);
        assert!(log.is_destroyed());
    }

    #[test]
    fn test_chain_verification_valid() {
        let mut log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:alice".to_string(),
            test_rf("did:omnia:alice"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        log.transfer(
            "did:omnia:bob".to_string(),
            test_rf("did:omnia:bob"),
            test_commitment(b"transfer1"),
        );

        log.transfer(
            "did:omnia:charlie".to_string(),
            test_rf("did:omnia:charlie"),
            test_commitment(b"transfer2"),
        );

        assert!(log.verify_chain());
    }

    #[test]
    fn test_chain_serialization_roundtrip() {
        let mut log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:alice".to_string(),
            test_rf("did:omnia:alice"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        log.transfer(
            "did:omnia:bob".to_string(),
            test_rf("did:omnia:bob"),
            test_commitment(b"transfer1"),
        );

        let bytes = log.to_bytes().unwrap();
        let restored = ProvenanceLog::from_bytes(&bytes).unwrap();

        assert_eq!(log.item_id, restored.item_id);
        assert_eq!(log.current_holder, restored.current_holder);
        assert_eq!(log.events.len(), restored.events.len());
    }

    #[test]
    fn test_multiple_transfers() {
        let mut log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:factory".to_string(),
            test_rf("did:omnia:factory"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        let holders = ["did:omnia:distributor", "did:omnia:retailer", "did:omnia:customer"];

        for (i, holder) in holders.iter().enumerate() {
            log.transfer(
                holder.to_string(),
                test_rf(holder),
                test_commitment(format!("transfer{}", i + 1).as_bytes()),
            );
        }

        assert_eq!(log.len(), 4); // 1 creation + 3 transfers
        assert_eq!(log.current_holder, "did:omnia:customer");
        assert!(log.verify_chain());
    }

    #[test]
    fn test_is_destroyed_false_for_active_item() {
        let log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:alice".to_string(),
            test_rf("did:omnia:alice"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        assert!(!log.is_destroyed());
    }

    /// Construct a valid chain, tamper with an entry, and assert
    /// that `verify_chain()` returns `false`.
    #[test]
    fn test_tampered_chain_detection() {
        let mut log = ProvenanceLog::new(
            test_item_id(),
            "did:omnia:factory".to_string(),
            test_rf("did:omnia:factory"),
            test_commitment(b"creation"),
            test_anchor(),
        );

        log.transfer(
            "did:omnia:distributor".to_string(),
            test_rf("did:omnia:distributor"),
            test_commitment(b"transfer1"),
        );

        log.transfer(
            "did:omnia:retailer".to_string(),
            test_rf("did:omnia:retailer"),
            test_commitment(b"transfer2"),
        );

        // Sanity: the untampered chain should verify
        assert!(log.verify_chain());

        // --- Tamper scenario 1: modify the data_hash of event 1 ---
        let mut tampered = log.clone();
        tampered.events[1].commitment.data_hash = [0xDEu8; 32];
        assert!(
            !tampered.verify_chain(),
            "tampering with data_hash must break the chain"
        );

        // --- Tamper scenario 2: swap two events ---
        let mut swapped = log.clone();
        let ev2 = swapped.events[2].clone();
        let ev1 = swapped.events[1].clone();
        swapped.events[1] = ev2;
        swapped.events[2] = ev1;
        assert!(!swapped.verify_chain(), "swapping events must break the chain");

        // --- Tamper scenario 3: corrupt the entry_hash ---
        let mut corrupt_hash = log.clone();
        corrupt_hash.events[1].entry_hash = [0xBAu8; 32];
        assert!(
            !corrupt_hash.verify_chain(),
            "corrupting entry_hash must break the chain"
        );

        // --- Tamper scenario 4: insert a fake event ---
        let mut inserted = log.clone();
        let fake = ProvenanceEvent {
            event_type: ProvenanceEventType::Verified,
            from: None,
            to: "did:omnia:attacker".to_string(),
            rf_proof: test_rf("did:omnia:attacker"),
            commitment: test_commitment(b"fake"),
            previous_hash: [0u8; 32], // wrong — should be prev.entry_hash
            entry_hash: [0u8; 32],    // wrong
            timestamp: VectorClock::new(),
        };
        inserted.events.insert(1, fake);
        assert!(!inserted.verify_chain(), "inserting a fake event must break the chain");

        // --- Tamper scenario 5: corrupt commitment.previous_hash ---
        let mut corrupt_prev = log.clone();
        corrupt_prev.events[2].commitment.previous_hash = [0xFFu8; 32];
        assert!(
            !corrupt_prev.verify_chain(),
            "corrupting commitment.previous_hash must break the chain"
        );
    }
}
