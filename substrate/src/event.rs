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

use crate::vector_clock::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Unique identifier for an event (SHA-256 hash of event content)
pub type EventId = [u8; 32];

/// A signature proving event authenticity
pub type Signature = Vec<u8>;

/// Payload data attached to an event (opaque to the substrate)
pub type Payload = Vec<u8>;

/// Status of an event in the consensus lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventStatus {
    /// Newly created, not yet propagated
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
    /// Cryptographic signature over the event hash
    pub signature: Signature,
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
    ///
    /// # Arguments
    /// * `creator` - The node creating this event
    /// * `sequence` - Monotonic sequence number for this creator
    /// * `vector_clock` - Current vector clock state
    /// * `self_parent` - Previous event from this creator (None for first event)
    /// * `other_parent` - Event received from another node (None for genesis)
    /// * `payload` - Application data
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
            signature: Vec::new(),
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

    /// Compute the SHA-256 hash of this event (used as its ID)
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
        hasher.finalize().into()
    }

    /// Verify that the event ID matches its content (integrity check)
    pub fn verify_hash(&self) -> bool {
        self.id == self.compute_hash()
    }

    /// Attach a cryptographic signature to this event
    pub fn sign(&mut self, signature: Signature) {
        self.signature = signature;
    }

    /// Verify the event's signature (placeholder — actual crypto in identity layer)
    pub fn verify_signature(&self) -> bool {
        // TODO: Integrate with Layer 4 Identity for real signature verification
        // For now, accept any non-empty signature
        !self.signature.is_empty()
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

    /// Full validation: hash integrity + signature
    pub fn validate(&self) -> Result<(), EventValidationError> {
        if !self.verify_hash() {
            return Err(EventValidationError::InvalidHash);
        }
        if !self.verify_signature() {
            return Err(EventValidationError::InvalidSignature);
        }
        if self.status == EventStatus::Rejected {
            return Err(EventValidationError::RejectedEvent);
        }
        Ok(())
    }

    /// Serialize event to bytes for network transmission
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize event from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EventValidationError> {
        bincode::deserialize(bytes).map_err(|_| EventValidationError::DeserializationError)
    }

    /// Mark this event as having received an acknowledgment
    pub fn add_acknowledgment(&mut self) {
        self.ack_count += 1;
        if self.ack_count >= 2 {
            // Will be updated to actual threshold by consensus module
            self.status = EventStatus::Acknowledged;
        }
    }

    /// Check if this event is a root (no parents)
    pub fn is_root(&self) -> bool {
        self.self_parent.is_none() && self.other_parent.is_none()
    }

    /// Check if this event links to another event (is in its ancestry)
    pub fn links_to(&self, event_id: &EventId) -> bool {
        self.self_parent
            .map(|p| &p == event_id)
            .unwrap_or(false)
            || self
                .other_parent
                .map(|p| &p == event_id)
                .unwrap_or(false)
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
    use crate::vector_clock::VectorClock;

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
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
        assert!(!event.is_root()); // has vector clock set, so not a "root" in genesis sense
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
    fn test_event_signature() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let mut event = Event::new(creator, 0, vc, None, None, vec![]);

        // Without signature, validation should fail
        assert!(matches!(
            event.validate(),
            Err(EventValidationError::InvalidSignature)
        ));

        // With signature, validation should pass
        event.sign(vec![1, 2, 3, 4]);
        assert!(event.validate().is_ok());
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
        let mut event = Event::new(creator, 0, vc, None, None, vec![]);
        event.sign(vec![1]);

        assert_eq!(event.status, EventStatus::Pending);
        event.add_acknowledgment();
        assert_eq!(event.ack_count, 1);
        event.add_acknowledgment();
        assert_eq!(event.status, EventStatus::Acknowledged);
    }
}

// Simple bincode reimplementation for no-std compatibility
mod bincode {
    pub fn serialize<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, ()> {
        serde_json::to_vec(value).map_err(|_| ())
    }

    pub fn deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ()> {
        serde_json::from_slice(bytes).map_err(|_| ())
    }
}
