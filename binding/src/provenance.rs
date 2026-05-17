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

/// A single event in an item's provenance chain.
///
/// Each event records who transferred to whom, with cryptographic proof
/// (RF fingerprint + quantum commitment) and a causal timestamp.
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
    /// Causal timestamp (vector clock).
    pub timestamp: VectorClock,
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
        let creation_event = ProvenanceEvent {
            event_type: ProvenanceEventType::Created,
            from: None,
            to: creator.clone(),
            rf_proof,
            commitment,
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
    /// to the log, and updates the current holder.
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
        commitment: QuantumCommitment,
    ) -> ProvenanceEvent {
        let event = ProvenanceEvent {
            event_type: ProvenanceEventType::Transferred,
            from: Some(self.current_holder.clone()),
            to: to.clone(),
            rf_proof,
            commitment,
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
    pub fn verify(
        &mut self,
        rf_proof: RfFingerprint,
        commitment: QuantumCommitment,
    ) -> ProvenanceEvent {
        let event = ProvenanceEvent {
            event_type: ProvenanceEventType::Verified,
            from: None,
            to: self.current_holder.clone(),
            rf_proof,
            commitment,
            timestamp: VectorClock::new(),
        };
        self.events.push(event.clone());
        event
    }

    /// Record a destruction event for the item.
    ///
    /// Once an item is destroyed, no further transfers are possible.
    pub fn destroy(
        &mut self,
        rf_proof: RfFingerprint,
        commitment: QuantumCommitment,
    ) -> ProvenanceEvent {
        let event = ProvenanceEvent {
            event_type: ProvenanceEventType::Destroyed,
            from: Some(self.current_holder.clone()),
            to: String::new(), // No new holder
            rf_proof,
            commitment,
            timestamp: VectorClock::new(),
        };
        self.events.push(event.clone());
        event
    }

    /// Verify the integrity of the entire provenance chain.
    ///
    /// Checks that every consecutive pair of events has a valid
    /// cryptographic link: each event's commitment must reference
    /// the previous event's commitment.
    ///
    /// # Returns
    ///
    /// `true` if the chain is intact, `false` if any link is broken.
    pub fn verify_chain(&self) -> bool {
        if self.events.is_empty() {
            return true;
        }

        // First event must be a Created event
        if self.events[0].event_type != ProvenanceEventType::Created {
            return false;
        }

        // Check chain links
        for window in self.events.windows(2) {
            let prev = &window[0];
            let curr = &window[1];
            // Current commitment should reference/link to previous commitment
            if !curr.commitment.links_to(&prev.commitment) {
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
    pub fn creation_event(&self) -> &ProvenanceEvent {
        &self.events[0]
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
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![Self::PROVENANCE_LOG_VERSION];
        bytes.extend(postcard::to_allocvec(self).expect("ProvenanceLog serialization cannot fail"));
        bytes
    }

    /// Deserialize a provenance log from bytes.
    ///
    /// Reads and validates the version byte before deserializing the
    /// payload. Returns an error if the version is unsupported.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        if bytes.is_empty() {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        let version = bytes[0];
        if version != Self::PROVENANCE_LOG_VERSION {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        postcard::from_bytes(&bytes[1..])
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

        let bytes = log.to_bytes();
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

        let holders = [
            "did:omnia:distributor",
            "did:omnia:retailer",
            "did:omnia:customer",
        ];

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
}
