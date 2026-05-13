//! Core Event Type
//!
//! Events are the fundamental unit of the Omnia protocol. Unlike blockchain blocks,
//! events form a DAG structure where each event references two parent events:
//! - Self-parent: The creator's previous event (forms a chain per node)
//! - Other-parent: An event received from another node (links the DAG)
//!
//! This two-parent structure (inspired by Hashgraph) enables:
//! - Causal tracking through vector clocks
//! - Parallel event creation (no single leader)
//! - Complete network history through graph traversal

use crate::crypto::{NodeKeypair, NodePublicKey, Signature as EdSignature, Signer, Verifier};
use crate::vector_clock::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum allowed clock drift for event timestamps (5 minutes in milliseconds).
/// Events with timestamps more than this far in the future are rejected.
pub const MAX_TIMESTAMP_DRIFT_MS: u64 = 300_000;

/// Maximum age for an event before it is considered ancient (365 days in milliseconds).
/// Events older than this are rejected as unreasonably stale.
pub const MAX_EVENT_AGE_MS: u64 = 31_536_000_000;

/// Unique identifier for an event (SHA-256 hash of event content)
pub type EventId = [u8; 32];

/// Payload data attached to an event (opaque to the substrate)
pub type Payload = Vec<u8>;

/// Status of an event in the consensus lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EventStatus {
    /// Newly created, not yet propagated
    #[default]
    Pending,
    /// Gossiped to at least one peer
    Gossiped,
    /// Received acknowledgments from >2/3 of nodes
    Acknowledged,
    /// Part of the finalized causal order
    Finalized,
    /// Rejected (invalid signature, double-spend, etc.)
    Rejected,
}

/// Serde helper for serializing `[u8; 64]` as bytes.
/// Serde only natively implements Serialize/Deserialize for arrays up to size 32.
mod serde_array_64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(data)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let bytes: Vec<u8> = Vec::deserialize(d)?;
        let mut arr = [0u8; 64];
        if bytes.len() == 64 {
            arr.copy_from_slice(&bytes);
            Ok(arr)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected 64 bytes, got {}",
                bytes.len()
            )))
        }
    }
}

/// The core Event struct — the fundamental unit of the Omnia DAG
///
/// Each event is immutable once created. Its hash (EventId) serves as
/// its unique identifier in the causal graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Unique event identifier (SHA-256 hash of all fields except signature)
    pub id: EventId,
    /// The node that created this event
    pub creator: NodeId,
    /// Monotonic sequence number from this creator (0-indexed)
    pub sequence: u64,
    /// Wall-clock timestamp (for reference only, not used for ordering)
    pub timestamp: u64,
    /// Vector clock at the time of creation (causal ordering)
    pub vector_clock: VectorClock,
    /// Hash of the creator's previous event (self-parent)
    pub self_parent: Option<EventId>,
    /// Hash of an event received from another node (other-parent)
    pub other_parent: Option<EventId>,
    /// Application-specific payload (transactions, etc.)
    pub payload: Payload,
    /// Ed25519 public key of the creator (32 bytes)
    pub creator_pubkey: [u8; 32],
    /// Ed25519 signature over the event hash (64 bytes)
    #[serde(with = "serde_array_64")]
    pub signature: [u8; 64],
    /// Current consensus status
    #[serde(skip)]
    pub status: EventStatus,
    /// Number of acknowledgments received (for consensus tracking)
    #[serde(skip)]
    pub ack_count: u32,
}

/// Lightweight event header for efficient gossip
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventHeader {
    pub id: EventId,
    pub creator: NodeId,
    pub sequence: u64,
    pub timestamp: u64,
    pub vector_clock: VectorClock,
    pub self_parent: Option<EventId>,
    pub other_parent: Option<EventId>,
}

/// Request for missing events (used during sync)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRequest {
    /// Events we already have (to avoid re-sending)
    pub known_events: Vec<EventId>,
    /// Maximum number of events to return
    pub limit: usize,
    /// Starting from this vector clock (inclusive)
    pub since: VectorClock,
}

/// Response to an event request
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBatch {
    pub events: Vec<Event>,
    /// Whether there are more events available
    pub has_more: bool,
    /// Vector clock representing the tip of the sender's graph
    pub tip_clock: VectorClock,
}

impl Event {
    /// Create a new event (without signature — must be signed separately)
    pub fn new(
        creator: NodeId,
        sequence: u64,
        vector_clock: VectorClock,
        self_parent: Option<EventId>,
        other_parent: Option<EventId>,
        payload: Payload,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut event = Self {
            id: [0u8; 32],
            creator,
            sequence,
            timestamp,
            vector_clock,
            self_parent,
            other_parent,
            payload,
            creator_pubkey: [0u8; 32],
            signature: [0u8; 64],
            status: EventStatus::Pending,
            ack_count: 0,
        };

        event.id = event.compute_hash();
        event
    }

    /// Create a genesis event (first event in the network)
    pub fn genesis(creator: NodeId, payload: Payload) -> Self {
        let vector_clock = VectorClock::with_node(creator, 1);
        Self::new(creator, 0, vector_clock, None, None, payload)
    }

    /// Compute the SHA-256 hash of this event (used as its ID).
    /// Includes creator_pubkey in the hash input for binding.
    fn compute_hash(&self) -> EventId {
        let mut hasher = Sha256::new();
        hasher.update(&self.creator);
        hasher.update(&self.sequence.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.vector_clock.to_bytes());
        if let Some(sp) = self.self_parent {
            hasher.update(&sp);
        }
        if let Some(op) = self.other_parent {
            hasher.update(&op);
        }
        hasher.update(&self.payload);
        hasher.update(&self.creator_pubkey);
        hasher.finalize().into()
    }

    /// Verify that the event ID matches its content (integrity check)
    pub fn verify_hash(&self) -> bool {
        self.id == self.compute_hash()
    }

    /// Sign this event with an Ed25519 keypair.
    /// Stores the public key and signature in the event.
    pub fn sign_with_keypair(&mut self, keypair: &NodeKeypair) {
        self.creator_pubkey = keypair.verifying_key().to_bytes();
        // Recompute hash now that pubkey is set
        self.id = self.compute_hash();
        let sig = keypair.sign(&self.id);
        self.signature = sig.to_bytes();
    }

    /// Verify the event's Ed25519 signature.
    /// Returns true only if the signature is cryptographically valid.
    pub fn verify_signature(&self) -> bool {
        let Ok(pubkey) = NodePublicKey::from_bytes(&self.creator_pubkey) else {
            return false;
        };
        let Ok(sig) = EdSignature::from_slice(&self.signature) else {
            return false;
        };
        pubkey.verify(&self.id, &sig).is_ok()
    }

    /// Legacy sign method for backward compatibility in tests.
    /// In production, always use `sign_with_keypair`.
    pub fn sign(&mut self, _signature: Vec<u8>) {
        // No-op: real signing requires a keypair.
        // Tests should migrate to `sign_with_keypair`.
    }

    /// Get the event header (lightweight metadata)
    pub fn header(&self) -> EventHeader {
        EventHeader {
            id: self.id,
            creator: self.creator,
            sequence: self.sequence,
            timestamp: self.timestamp,
            vector_clock: self.vector_clock.clone(),
            self_parent: self.self_parent,
            other_parent: self.other_parent,
        }
    }

    /// Full validation: unsigned check, hash integrity, signature, timestamp sanity, and rejection status.
    ///
    /// Checks are performed in order of increasing cost:
    /// 1. Unsigned event (zero signature or pubkey)
    /// 2. Hash integrity
    /// 3. Cryptographic signature
    /// 4. Future timestamp (beyond MAX_TIMESTAMP_DRIFT_MS)
    /// 5. Ancient timestamp (older than MAX_EVENT_AGE_MS)
    /// 6. Rejected status
    pub fn validate(&self) -> Result<(), EventValidationError> {
        // Check for unsigned events: all-zero signature or pubkey means never signed
        if self.signature == [0u8; 64] || self.creator_pubkey == [0u8; 32] {
            return Err(EventValidationError::UnsignedEvent);
        }
        if !self.verify_hash() {
            return Err(EventValidationError::InvalidHash);
        }
        if !self.verify_signature() {
            return Err(EventValidationError::InvalidSignature);
        }

        // Timestamp sanity checks
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Reject events too far in the future
        if let Some(max_allowed) = now_ms.checked_add(MAX_TIMESTAMP_DRIFT_MS) {
            if self.timestamp > max_allowed {
                return Err(EventValidationError::FutureTimestamp);
            }
        }

        // Reject events that are unreasonably old
        if let Some(oldest_allowed) = now_ms.checked_sub(MAX_EVENT_AGE_MS) {
            if self.timestamp < oldest_allowed {
                return Err(EventValidationError::AncientTimestamp);
            }
        }

        if self.status == EventStatus::Rejected {
            return Err(EventValidationError::RejectedEvent);
        }
        Ok(())
    }

    /// Serialize event to compact binary bytes for network transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("Event serialization cannot fail")
    }

    /// Deserialize event from compact binary bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EventValidationError> {
        bincode::deserialize(bytes).map_err(|_| EventValidationError::DeserializationError)
    }

    /// Mark this event as having received an acknowledgment
    pub fn add_acknowledgment(&mut self) {
        self.ack_count += 1;
        if self.ack_count >= 2 {
            self.status = EventStatus::Acknowledged;
        }
    }

    /// Check if this event is a root (no parents)
    pub fn is_root(&self) -> bool {
        self.self_parent.is_none() && self.other_parent.is_none()
    }

    /// Check if this event links to another event (is in its ancestry)
    pub fn links_to(&self, event_id: &EventId) -> bool {
        self.self_parent.map(|p| &p == event_id).unwrap_or(false)
            || self.other_parent.map(|p| &p == event_id).unwrap_or(false)
    }
}

impl EventHeader {
    /// Check if two events are causally related
    pub fn is_ancestor_of(&self, other: &EventHeader) -> bool {
        other.vector_clock.happened_after(&self.vector_clock)
    }

    /// Check if two events are concurrent (independent)
    pub fn is_concurrent_with(&self, other: &EventHeader) -> bool {
        self.vector_clock.concurrent(&other.vector_clock)
    }
}

/// Errors during event validation
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventValidationError {
    #[error("Event hash does not match content")]
    InvalidHash,
    #[error("Event signature is invalid")]
    InvalidSignature,
    #[error("Event has been rejected")]
    RejectedEvent,
    #[error("Failed to deserialize event")]
    DeserializationError,
    #[error("Event timestamp is too far in the future")]
    FutureTimestamp,
    #[error("Event timestamp is unreasonably old")]
    AncientTimestamp,
    #[error("Event is unsigned (zero signature or pubkey)")]
    UnsignedEvent,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let short_id = hex::encode(&self.id[..4]);
        let short_creator = hex::encode(&self.creator[..4]);
        write!(
            f,
            "Event[{}] creator={} seq={} status={:?}",
            short_id, short_creator, self.sequence, self.status
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::generate_keypair;
    use crate::vector_clock::VectorClock;
    use rand::rngs::OsRng;

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn test_keypair() -> NodeKeypair {
        NodeKeypair::generate(&mut OsRng)
    }

    #[test]
    fn test_event_creation() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]);

        assert_eq!(event.creator, creator);
        assert_eq!(event.sequence, 0);
        assert!(event.self_parent.is_none());
        assert!(event.other_parent.is_none());
        assert!(event.is_root());
    }

    #[test]
    fn test_genesis_event() {
        let creator = test_node(1);
        let event = Event::genesis(creator, vec![]);

        assert!(event.is_root());
        assert_eq!(event.sequence, 0);
    }

    #[test]
    fn test_event_hash_integrity() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]);

        assert!(event.verify_hash());
    }

    #[test]
    fn test_event_signature_real_crypto() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]);

        // Before signing, the creator_pubkey is all zeros and signature is all zeros.
        // verify_signature() may return true for the all-zeros key/signature pair
        // (they form a valid ed25519 pair), so we test the actual signing flow instead.

        // Sign with real keypair
        event.sign_with_keypair(&keypair);
        assert!(event.verify_signature());
        assert!(event.validate().is_ok());

        // Tamper with the event ID should invalidate signature
        let mut tampered = event.clone();
        tampered.id = [99u8; 32]; // Corrupt the ID
        assert!(!tampered.verify_signature());
    }

    #[test]
    fn test_event_parent_links() {
        let n1 = test_node(1);
        let n2 = test_node(2);

        let genesis = Event::genesis(n1, vec![]);
        let genesis_id = genesis.id;

        let mut vc = VectorClock::with_node(n1, 2);
        vc.set(n2, 1);
        let event = Event::new(n1, 1, vc, Some(genesis_id), None, vec![]);

        assert!(event.links_to(&genesis_id));
    }

    #[test]
    fn test_acknowledgment_tracking() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]);
        event.sign_with_keypair(&keypair);

        assert_eq!(event.status, EventStatus::Pending);
        event.add_acknowledgment();
        assert_eq!(event.ack_count, 1);
        event.add_acknowledgment();
        assert_eq!(event.status, EventStatus::Acknowledged);
    }

    #[test]
    fn test_validate_unsigned_event() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        // Event::new creates an event with all-zero signature and pubkey
        let event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]);

        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::UnsignedEvent));
    }

    #[test]
    fn test_validate_future_timestamp() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]);

        // Set timestamp 10 minutes in the future (beyond 5-minute drift)
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        event.timestamp = now_ms + 600_000; // 10 minutes ahead
        event.sign_with_keypair(&keypair);

        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::FutureTimestamp));
    }

    #[test]
    fn test_validate_ancient_timestamp() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]);

        // Set timestamp to 2 years ago (well beyond 1-year max age)
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let two_years_ms: u64 = 2 * 365 * 24 * 60 * 60 * 1000;
        event.timestamp = now_ms - two_years_ms;
        event.sign_with_keypair(&keypair);

        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::AncientTimestamp));
    }

    #[test]
    fn test_validate_tampered_payload() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

        // Tamper with the payload — hash check should fail
        let mut tampered = event.clone();
        tampered.payload = vec![9, 9, 9];
        // The id still matches the original, but compute_hash will differ
        let result = tampered.validate();
        assert_eq!(result, Err(EventValidationError::InvalidHash));
    }

    #[test]
    fn test_validate_rejected_event() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]);
        event.sign_with_keypair(&keypair);
        event.status = EventStatus::Rejected;

        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::RejectedEvent));
    }

    #[test]
    fn test_validate_valid_signed_event() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![4, 5, 6]);
        event.sign_with_keypair(&keypair);

        // A freshly signed event with recent timestamp should pass all checks
        let result = event.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_event_serialization_roundtrip() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

        let bytes = event.to_bytes();
        let restored = Event::from_bytes(&bytes).unwrap();
        assert_eq!(event.id, restored.id);
        assert_eq!(event.signature, restored.signature);
        assert_eq!(event.creator_pubkey, restored.creator_pubkey);
    }
}
