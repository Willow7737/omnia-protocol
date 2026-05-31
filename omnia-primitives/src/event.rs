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

use crate::blake3_domain::blake3_hash_domain;
use crate::vector_clock::{NodeId, VectorClock};
use ed25519_dalek::{
    Signature as EdSignature, Signer, SigningKey as NodeKeypair, Verifier, VerifyingKey as NodePublicKey,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

#[cfg(feature = "legacy-hash")]
use sha2::{Digest, Sha256};

/// Maximum allowed clock drift for event timestamps (2 minutes in milliseconds).
/// Events with timestamps more than this far in the future are rejected.
pub const MAX_TIMESTAMP_DRIFT_MS: u64 = 120_000; // 2 minutes (was 5 min)

/// Maximum age for an event before it is considered ancient (365 days in milliseconds).
/// Events older than this are rejected as unreasonably stale.
pub const MAX_EVENT_AGE_MS: u64 = 31_536_000_000;

/// Unique identifier for an event (BLAKE3 hash of event content)
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
    /// Event identifier
    pub id: EventId,
    /// Creator node identifier
    pub creator: NodeId,
    /// Sequence number from the creator
    pub sequence: u64,
    /// Wall-clock timestamp
    pub timestamp: u64,
    /// Vector clock for causal ordering
    pub vector_clock: VectorClock,
    /// Hash of the creator's previous event
    pub self_parent: Option<EventId>,
    /// Hash of an event from another node
    pub other_parent: Option<EventId>,
}

/// Maximum number of events that can be requested in a single EventRequest
pub const MAX_EVENT_REQUEST_LIMIT: usize = 10_000;
/// Maximum number of known events that can be listed in an EventRequest
pub const MAX_KNOWN_EVENTS: usize = 100_000;

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

impl EventRequest {
    /// Validate the request limits.
    ///
    /// Returns an error if `limit` exceeds [`MAX_EVENT_REQUEST_LIMIT`] or
    /// `known_events` exceeds [`MAX_KNOWN_EVENTS`].
    pub fn validate(&self) -> Result<(), EventValidationError> {
        if self.limit > MAX_EVENT_REQUEST_LIMIT {
            return Err(EventValidationError::InvalidField("limit exceeds maximum".into()));
        }
        if self.known_events.len() > MAX_KNOWN_EVENTS {
            return Err(EventValidationError::InvalidField(
                "known_events exceeds maximum".into(),
            ));
        }
        Ok(())
    }
}

/// Response to an event request
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBatch {
    /// Events in the batch
    pub events: Vec<Event>,
    /// Whether there are more events available
    pub has_more: bool,
    /// Vector clock representing the tip of the sender's graph
    pub tip_clock: VectorClock,
}

impl Event {
    /// Create a new event (without signature — must be signed separately).
    ///
    /// The event ID is computed as the SHA-256 hash of all fields except the
    /// signature. The timestamp is set to the current system time. The event
    /// is created with [`EventStatus::Pending`] and zero acknowledgment count.
    ///
    /// Call [`sign_with_keypair()`](Self::sign_with_keypair) after creation
    /// to attach a cryptographic signature and derive the creator identity.
    ///
    /// # Arguments
    ///
    /// * `creator` — The node ID of the event creator (overridden by `sign_with_keypair`)
    /// * `sequence` — Monotonic sequence number from this creator (0-indexed)
    /// * `vector_clock` — Causal state at the time of creation
    /// * `self_parent` — Hash of the creator's previous event (`None` for genesis)
    /// * `other_parent` — Hash of an event received from another node (`None` for genesis)
    /// * `payload` — Application-specific data attached to this event
    ///
    /// # Example
    ///
    /// ```ignore
    /// use omnia_substrate::{Event, VectorClock};
    /// let vc = VectorClock::with_node(creator, 1);
    /// let event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();
    /// assert!(event.is_root());
    /// ```
    pub fn new(
        creator: NodeId,
        sequence: u64,
        vector_clock: VectorClock,
        self_parent: Option<EventId>,
        other_parent: Option<EventId>,
        payload: Payload,
    ) -> Result<Self, EventValidationError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| EventValidationError::InvalidTimestamp)?
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
        Ok(event)
    }

    /// Create a genesis event (first event in the network).
    ///
    /// A genesis event has sequence 0, no parents, and a vector clock with
    /// only the creator's entry set to 1. It is the root of the DAG.
    ///
    /// # Arguments
    ///
    /// * `creator` — The node ID of the genesis event creator
    /// * `payload` — Application-specific data attached to this event
    pub fn genesis(creator: NodeId, payload: Payload) -> Result<Self, EventValidationError> {
        let vector_clock = VectorClock::with_node(creator, 1);
        Self::new(creator, 0, vector_clock, None, None, payload)
    }

    /// Compute the BLAKE3 hash of this event (used as its ID).
    /// Includes creator_pubkey in the hash input for binding.
    fn compute_hash(&self) -> EventId {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"OMNIA-EVENT-ID-V2"); // Domain separation
        hasher.update(&self.creator_pubkey);
        hasher.update(&self.sequence.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.payload);
        match self.self_parent {
            None => {
                hasher.update(&[0u8]);
            }
            Some(sp) => {
                hasher.update(&[1u8]);
                hasher.update(&sp);
            }
        }
        match self.other_parent {
            None => {
                hasher.update(&[0u8]);
            }
            Some(op) => {
                hasher.update(&[1u8]);
                hasher.update(&op);
            }
        }
        *hasher.finalize().as_bytes()
    }

    /// Compute the legacy SHA-256 hash (for backward compatibility).
    /// Only available with the `legacy-hash` feature flag.
    #[cfg(feature = "legacy-hash")]
    fn compute_hash_legacy(&self) -> EventId {
        let mut hasher = Sha256::new();
        hasher.update(self.creator);
        hasher.update(self.sequence.to_le_bytes());
        hasher.update(self.timestamp.to_le_bytes());
        hasher.update(self.vector_clock.to_bytes());
        match self.self_parent {
            None => hasher.update([0u8]),
            Some(sp) => {
                hasher.update([1u8]);
                hasher.update(sp);
            }
        }
        match self.other_parent {
            None => hasher.update([0u8]),
            Some(op) => {
                hasher.update([1u8]);
                hasher.update(op);
            }
        }
        hasher.update(&self.payload);
        hasher.update(&self.creator_pubkey);
        hasher.finalize().into()
    }

    /// Verify that the event ID matches its content (integrity check).
    ///
    /// Uses constant-time comparison to prevent timing side-channels
    /// that could leak information about the expected hash.
    pub fn verify_hash(&self) -> bool {
        let computed = self.compute_hash();
        self.id.ct_eq(&computed).into()
    }

    /// Sign this event with an Ed25519 keypair.
    ///
    /// Stores the public key in `creator_pubkey`, derives the `creator` field
    /// as `blake3_hash_domain(b"omnia-creator", creator_pubkey)` to bind identity to
    /// the signing key, recomputes the event hash, and then signs the hash with
    /// the private key.
    ///
    /// # Arguments
    ///
    /// * `keypair` — The Ed25519 keypair to sign with.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use omnia_substrate::{Event, generate_keypair};
    /// let keypair = generate_keypair();
    /// let mut event = Event::genesis([0u8; 32], vec![]).unwrap();
    /// event.sign_with_keypair(&keypair);
    /// assert!(event.validate().is_ok());
    /// ```
    pub fn sign_with_keypair(&mut self, keypair: &NodeKeypair) {
        self.creator_pubkey = keypair.verifying_key().to_bytes();
        // Derive creator from pubkey: creator = blake3_hash_domain("omnia-creator", creator_pubkey)
        self.creator = blake3_hash_domain(b"omnia-creator", &self.creator_pubkey);
        // Recompute hash now that pubkey and creator are set
        self.id = self.compute_hash();
        let sig = keypair.sign(&self.id);
        self.signature = sig.to_bytes();
    }

    /// Verify the event's Ed25519 signature.
    ///
    /// Returns `true` only if the signature is cryptographically valid
    /// against the stored `creator_pubkey`. Returns `false` if the public
    /// key or signature bytes are malformed.
    ///
    /// # Security
    ///
    /// This verifies the signature over the event ID (hash). A valid
    /// signature proves that the holder of the private key produced or
    /// approved this event. However, this check alone does **not** verify
    /// the creator-identity binding — use [`validate()`](Self::validate)
    /// for full validation including the binding check.
    pub fn verify_signature(&self) -> bool {
        let Ok(pubkey) = NodePublicKey::from_bytes(&self.creator_pubkey) else {
            return false;
        };
        let Ok(sig) = EdSignature::from_slice(&self.signature) else {
            return false;
        };
        pubkey.verify(&self.id, &sig).is_ok()
    }

    /// Legacy sign method — **this is intentionally a no-op**.
    ///
    /// Calling this method does nothing. It exists only for backward
    /// compatibility with older call sites. To actually sign an event,
    /// use [`sign_with_keypair()`](Self::sign_with_keypair) instead.
    #[deprecated(since = "0.2.0", note = "sign() is a no-op - use sign_with_keypair() instead")]
    pub fn sign(&mut self, _signature: Vec<u8>) {
        // This method is intentionally a no-op for backward compatibility.
        // Use sign_with_keypair() for actual signing.
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

    /// Validate the creator-identity binding.
    ///
    /// The `creator` field MUST be the domain-separated BLAKE3 hash of
    /// `creator_pubkey` with domain `b"omnia-creator"`.
    /// This prevents impersonation: a malicious actor cannot claim a different
    /// creator identity while signing with their own key.
    ///
    /// Uses constant-time comparison via `subtle::ConstantTimeEq` to prevent
    /// timing side-channel attacks that could leak information about the
    /// expected creator identity.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if `creator == blake3_hash_domain("omnia-creator", creator_pubkey)` (constant-time)
    /// * `Err(EventValidationError::CreatorPubkeyMismatch)` otherwise
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = event.validate_creator_binding();
    /// assert!(result.is_ok());
    /// ```
    fn validate_creator_binding(&self) -> Result<(), EventValidationError> {
        let expected_creator = blake3_hash_domain(b"omnia-creator", &self.creator_pubkey);
        if self.creator.ct_ne(&expected_creator).into() {
            return Err(EventValidationError::CreatorPubkeyMismatch {
                claimed: hex::encode(self.creator),
                derived: hex::encode(expected_creator),
            });
        }
        Ok(())
    }

    /// Full validation: unsigned check, creator binding, payload size, hash integrity,
    /// signature, timestamp sanity, and rejection status.
    ///
    /// Checks are performed in order of increasing cost:
    /// 1. Unsigned event (zero signature or pubkey)
    /// 2. Creator-pubkey binding (identity integrity)
    /// 3. Payload size limit
    /// 4. Hash integrity
    /// 5. Cryptographic signature
    /// 6. Future timestamp (beyond MAX_TIMESTAMP_DRIFT_MS)
    /// 7. Ancient timestamp (older than MAX_EVENT_AGE_MS)
    /// 8. Rejected status
    pub fn validate(&self) -> Result<(), EventValidationError> {
        // Check for unsigned events: all-zero signature or pubkey means never signed
        if self.signature == [0u8; 64] || self.creator_pubkey == [0u8; 32] {
            return Err(EventValidationError::UnsignedEvent);
        }
        // Verify creator-pubkey binding before expensive crypto checks
        self.validate_creator_binding()?;
        // Reject oversized payloads before hash/signature verification
        if self.payload.len() > MAX_PAYLOAD_SIZE {
            return Err(EventValidationError::PayloadTooLarge {
                size: self.payload.len(),
                max: MAX_PAYLOAD_SIZE,
            });
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
            .map_err(|_| EventValidationError::InvalidTimestamp)?
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
    ///
    /// The output is prefixed with a wire-format version byte to enable
    /// future format migrations. The current format uses `postcard` for
    /// deterministic, `no_std`-compatible encoding.
    ///
    /// # Errors
    ///
    /// Returns [`EventValidationError::SerializationError`] if postcard
    /// serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, EventValidationError> {
        crate::wire_format::serialize_with_version(self).map_err(|_| EventValidationError::SerializationError)
    }

    /// Deserialize event from compact binary bytes.
    ///
    /// Checks the wire-format version byte before deserializing.
    ///
    /// # Errors
    ///
    /// Returns [`EventValidationError::DeserializationError`] if the version
    /// byte is unsupported or postcard deserialization fails.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EventValidationError> {
        crate::wire_format::deserialize_with_version(bytes)
            .map_err(|e| EventValidationError::DeserializationError(format!("{e}")))
    }

    /// Mark this event as having received an acknowledgment with a configurable threshold.
    pub fn add_acknowledgment_with_threshold(&mut self, threshold: u32) {
        self.ack_count = self.ack_count.saturating_add(1);
        if self.ack_count >= threshold {
            self.status = EventStatus::Acknowledged;
        }
    }

    /// Mark this event as having received an acknowledgment.
    /// Uses the default threshold of 2 for backward compatibility.
    pub fn add_acknowledgment(&mut self) {
        self.add_acknowledgment_with_threshold(2);
    }

    /// Check if this event is a root (no parents)
    pub fn is_root(&self) -> bool {
        self.self_parent.is_none() && self.other_parent.is_none()
    }

    /// Check if this event directly links to another event as a parent
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

/// Maximum payload size: 1 MiB.
/// Events exceeding this size are rejected before processing.
/// This prevents DoS via oversized payloads.
pub const MAX_PAYLOAD_SIZE: usize = 1024 * 1024;

/// Errors during event validation
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventValidationError {
    #[error("Event hash does not match content")]
    /// Hash integrity check failed
    InvalidHash,
    #[error("Event signature is invalid")]
    /// Cryptographic signature is invalid
    InvalidSignature,
    #[error("Event has been rejected")]
    /// Event was previously rejected
    RejectedEvent,
    #[error("Failed to deserialize event: {0}")]
    /// Deserialization failed
    DeserializationError(String),
    #[error("Failed to serialize event")]
    /// Serialization failed
    SerializationError,
    #[error("Event timestamp is too far in the future")]
    /// Timestamp exceeds allowed drift
    FutureTimestamp,
    #[error("Event timestamp is unreasonably old")]
    /// Timestamp is too old
    AncientTimestamp,
    #[error("Event is unsigned (zero signature or pubkey)")]
    /// Event has no valid signature
    UnsignedEvent,
    #[error("Event parent references form a cycle")]
    /// Parent references create a cycle
    CircularParentReference,
    #[error("Event references a non-existent parent")]
    /// Referenced parent does not exist
    MissingParent,
    #[error("Event has obviously invalid data (zero amount)")]
    /// Obviously invalid data detected
    ZeroAmount,
    /// Creator identity does not match pubkey binding
    #[error("Creator identity does not match pubkey: claimed {claimed}, derived {derived}")]
    CreatorPubkeyMismatch {
        /// The claimed creator identity from the event
        claimed: String,
        /// The derived identity from blake3(creator_pubkey)
        derived: String,
    },
    /// Payload exceeds the maximum allowed size
    #[error("Payload too large: {size} bytes (max {max})")]
    PayloadTooLarge {
        /// Actual payload size in bytes
        size: usize,
        /// Maximum allowed payload size in bytes
        max: usize,
    },
    /// System clock is before Unix epoch
    #[error("system clock is before Unix epoch")]
    InvalidTimestamp,
    /// A field has an invalid value
    #[error("Invalid field: {0}")]
    InvalidField(String),
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::vector_clock::VectorClock;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn generate_keypair() -> SigningKey {
        let mut csprng = OsRng;
        SigningKey::generate(&mut csprng)
    }

    fn test_keypair() -> NodeKeypair {
        generate_keypair()
    }

    #[test]
    fn test_event_creation() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]).unwrap();

        assert_eq!(event.creator, creator);
        assert_eq!(event.sequence, 0);
        assert!(event.self_parent.is_none());
        assert!(event.other_parent.is_none());
        assert!(event.is_root());
    }

    #[test]
    fn test_genesis_event() {
        let creator = test_node(1);
        let event = Event::genesis(creator, vec![]).unwrap();

        assert!(event.is_root());
        assert_eq!(event.sequence, 0);
    }

    #[test]
    fn test_event_hash_integrity() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]).unwrap();

        assert!(event.verify_hash());
    }

    #[test]
    fn test_event_signature_real_crypto() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();

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

        let genesis = Event::genesis(n1, vec![]).unwrap();
        let genesis_id = genesis.id;

        let mut vc = VectorClock::with_node(n1, 2);
        vc.set(n2, 1);
        let event = Event::new(n1, 1, vc, Some(genesis_id), None, vec![]).unwrap();

        assert!(event.links_to(&genesis_id));
    }

    #[test]
    fn test_acknowledgment_tracking() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();
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
        let event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]).unwrap();

        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::UnsignedEvent));
    }

    #[test]
    fn test_validate_future_timestamp() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();

        // Set timestamp 10 minutes in the future (beyond 2-minute drift)
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
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();

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
        let mut event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]).unwrap();
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
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();
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
        let mut event = Event::new(creator, 0, vc, None, None, vec![4, 5, 6]).unwrap();
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
        let mut event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]).unwrap();
        event.sign_with_keypair(&keypair);

        let bytes = event.to_bytes().expect("test event serialization");
        let restored = Event::from_bytes(&bytes).expect("test event deserialization");
        assert_eq!(event.id, restored.id);
        assert_eq!(event.signature, restored.signature);
        assert_eq!(event.creator_pubkey, restored.creator_pubkey);
    }

    // ── Adversarial tests (Sprint 1, Task 1.2) ────────────────────────

    /// Test that a malformed (non-zero but incorrect) signature is rejected.
    /// This differs from the all-zero signature test: here the signature bytes
    /// are non-zero but do not correspond to the event hash under the claimed pubkey.
    #[test]
    fn test_validate_malformed_signature() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![1, 2, 3]).unwrap();
        event.sign_with_keypair(&keypair);

        // Replace signature with non-zero garbage that is still 64 bytes
        let mut tampered = event.clone();
        tampered.signature = [0xABu8; 64];
        // The hash is still valid (we didn't change the id), but the signature
        // won't verify against the pubkey for this event hash.
        let result = tampered.validate();
        assert_eq!(result, Err(EventValidationError::InvalidSignature));
    }

    /// Test that an event signed with one keypair but whose ID was then
    /// replaced with a different hash (simulating a tampered event ID with
    /// a valid-looking signature from a different context) is rejected.
    #[test]
    fn test_validate_tampered_id_with_valid_signature() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair_a = test_keypair();
        let keypair_b = test_keypair();

        // Sign with keypair A
        let mut event = Event::new(creator, 0, vc.clone(), None, None, vec![]).unwrap();
        event.sign_with_keypair(&keypair_a);

        // Now create a different event and sign with keypair B, then swap the ID
        let mut other_event = Event::new(creator, 1, vc, None, None, vec![9, 9, 9]).unwrap();
        other_event.sign_with_keypair(&keypair_b);

        // Swap the ID and signature from other_event into event (cross-contamination)
        let mut forged = event.clone();
        forged.id = other_event.id;
        // The hash won't match, so this should fail hash integrity first
        let result = forged.validate();
        assert_eq!(result, Err(EventValidationError::InvalidHash));
    }

    /// Test that an event with all-zero signature but non-zero pubkey is rejected.
    #[test]
    fn test_validate_zero_signature_nonzero_pubkey() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();
        event.sign_with_keypair(&keypair);

        // Zero out the signature but keep the pubkey
        event.signature = [0u8; 64];
        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::UnsignedEvent));
    }

    /// Test that an event with non-zero signature but all-zero pubkey is rejected.
    #[test]
    fn test_validate_nonzero_signature_zero_pubkey() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();
        event.sign_with_keypair(&keypair);

        // Zero out the pubkey but keep the signature
        event.creator_pubkey = [0u8; 32];
        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::UnsignedEvent));
    }

    /// Test that an event with a timestamp exactly at the MAX_TIMESTAMP_DRIFT_MS boundary
    /// (just barely too far in the future) is rejected. Uses a generous margin to
    /// avoid timing-dependent flakiness.
    #[test]
    fn test_validate_future_timestamp_at_boundary() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        // Set well beyond the drift limit (10 seconds margin to avoid race conditions)
        event.timestamp = now_ms + MAX_TIMESTAMP_DRIFT_MS + 10_000;
        event.sign_with_keypair(&keypair);

        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::FutureTimestamp));
    }

    /// Test that an event with a timestamp exactly at the MAX_EVENT_AGE_MS boundary
    /// (just barely too old) is rejected.
    #[test]
    fn test_validate_ancient_timestamp_at_boundary() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![]).unwrap();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        // Set exactly 1ms older than the max age
        event.timestamp = now_ms - MAX_EVENT_AGE_MS - 1;
        event.sign_with_keypair(&keypair);

        let result = event.validate();
        assert_eq!(result, Err(EventValidationError::AncientTimestamp));
    }

    /// Test that the new CircularParentReference error variant exists and is constructible.
    #[test]
    fn test_circular_parent_reference_error_variant() {
        let err = EventValidationError::CircularParentReference;
        assert_eq!(format!("{err}"), "Event parent references form a cycle");
    }

    /// Test that the new MissingParent error variant exists and is constructible.
    #[test]
    fn test_missing_parent_error_variant() {
        let err = EventValidationError::MissingParent;
        assert_eq!(format!("{err}"), "Event references a non-existent parent");
    }

    /// Test that the new ZeroAmount error variant exists and is constructible.
    #[test]
    fn test_zero_amount_error_variant() {
        let err = EventValidationError::ZeroAmount;
        assert_eq!(format!("{err}"), "Event has obviously invalid data (zero amount)");
    }

    /// Test that an event whose payload was tampered after signing fails validation
    /// with InvalidHash (more adversarial variant with different tamper strategy).
    #[test]
    fn test_validate_subtly_tampered_payload() {
        let creator = test_node(1);
        let vc = VectorClock::with_node(creator, 1);
        let keypair = test_keypair();
        let mut event = Event::new(creator, 0, vc, None, None, vec![0u8; 100]).unwrap();
        event.sign_with_keypair(&keypair);

        // Flip a single bit in the payload
        let mut tampered = event.clone();
        tampered.payload[50] ^= 0x01;
        let result = tampered.validate();
        assert_eq!(result, Err(EventValidationError::InvalidHash));
    }

    // ── Creator-binding tests (Sprint 4, Task A1) ─────────────────────

    /// Test that a valid event (creator == blake3_hash_domain("omnia-creator", creator_pubkey)) validates successfully.
    /// This would have passed before A1 too, but now it explicitly checks the binding.
    #[test]
    fn test_validate_creator_binding_valid() {
        let keypair = test_keypair();
        let creator_from_new = test_node(1); // arbitrary, will be overwritten
        let vc = VectorClock::with_node(creator_from_new, 1);
        let mut event = Event::new(creator_from_new, 0, vc, None, None, vec![1, 2, 3]).unwrap();
        event.sign_with_keypair(&keypair);

        // After sign_with_keypair, creator == blake3_hash_domain("omnia-creator", creator_pubkey)
        let expected = blake3_hash_domain(b"omnia-creator", &event.creator_pubkey);
        assert_eq!(event.creator, expected);

        let result = event.validate();
        assert!(result.is_ok());
    }

    /// Test that a mismatched creator (creator != blake3(creator_pubkey)) is rejected
    /// with CreatorPubkeyMismatch. This test would have FAILED before A1 was implemented.
    #[test]
    fn test_validate_creator_binding_mismatch() {
        let keypair = test_keypair();
        let fake_creator = test_node(99); // not derived from the keypair
        let vc = VectorClock::with_node(fake_creator, 1);
        let mut event = Event::new(fake_creator, 0, vc, None, None, vec![1, 2, 3]).unwrap();
        event.sign_with_keypair(&keypair);

        // sign_with_keypair now sets creator = blake3(creator_pubkey), so we
        // must manually tamper the creator field to simulate the old vulnerability.
        let mut tampered = event.clone();
        tampered.creator = fake_creator; // override with a non-derived identity
                                         // Recompute hash so the tampered event passes hash integrity
        tampered.id = tampered.compute_hash();
        // Re-sign with the keypair to fix the signature
        let sig = keypair.sign(&tampered.id);
        tampered.signature = sig.to_bytes();

        let result = tampered.validate();
        assert!(
            matches!(result, Err(EventValidationError::CreatorPubkeyMismatch { .. })),
            "Expected CreatorPubkeyMismatch, got {result:?}"
        );
    }

    /// Test that an event created through sign_with_keypair automatically has
    /// the correct creator binding (creator == blake3_hash_domain("omnia-creator", creator_pubkey)).
    #[test]
    fn test_sign_with_keypair_sets_correct_creator_binding() {
        let keypair = test_keypair();
        let arbitrary_creator = test_node(42);
        let vc = VectorClock::with_node(arbitrary_creator, 1);
        let mut event = Event::new(arbitrary_creator, 0, vc, None, None, vec![]).unwrap();

        // Before signing, creator is whatever was passed to Event::new
        assert_eq!(event.creator, arbitrary_creator);

        event.sign_with_keypair(&keypair);

        // After signing, creator MUST be blake3_hash_domain("omnia-creator", creator_pubkey)
        let expected_creator = blake3_hash_domain(b"omnia-creator", &event.creator_pubkey);
        assert_eq!(event.creator, expected_creator);
        assert_ne!(event.creator, arbitrary_creator); // ensure it was actually changed
    }

    /// Test that manually tampering the creator field after signing causes validation to fail.
    /// This is the core impersonation scenario that A1 prevents.
    #[test]
    fn test_tampered_creator_fails_validation() {
        let keypair = test_keypair();
        let vc = VectorClock::with_node(test_node(1), 1);
        let mut event = Event::new(test_node(1), 0, vc, None, None, vec![]).unwrap();
        event.sign_with_keypair(&keypair);

        // Tamper: change creator to something else
        let mut tampered = event.clone();
        tampered.creator = test_node(7);
        // Don't recompute hash or resign — just check that the binding is broken
        let result = tampered.validate();
        // Should fail on either creator binding or hash integrity
        assert!(result.is_err(), "Tampered creator should fail validation");
    }

    // ── Payload size limit tests (Sprint 4, Task A2) ──────────────────

    /// Test that an event with payload at exactly MAX_PAYLOAD_SIZE accepts.
    #[test]
    fn test_payload_at_max_size_accepts() {
        let keypair = test_keypair();
        let creator = blake3_hash_domain(b"omnia-creator", &keypair.verifying_key().to_bytes());
        let vc = VectorClock::with_node(creator, 1);
        let mut event = Event::new(creator, 0, vc, None, None, vec![0u8; MAX_PAYLOAD_SIZE]).unwrap();
        event.sign_with_keypair(&keypair);

        let result = event.validate();
        assert!(
            result.is_ok(),
            "Payload at exactly MAX_PAYLOAD_SIZE should accept, got {result:?}"
        );
    }

    /// Test that an event with payload = MAX_PAYLOAD_SIZE + 1 is rejected with PayloadTooLarge.
    /// This test would have FAILED before A2 was implemented.
    #[test]
    fn test_payload_exceeds_max_size_rejects() {
        let keypair = test_keypair();
        let creator = blake3_hash_domain(b"omnia-creator", &keypair.verifying_key().to_bytes());
        let vc = VectorClock::with_node(creator, 1);
        let oversized = vec![0u8; MAX_PAYLOAD_SIZE + 1];
        let mut event = Event::new(creator, 0, vc, None, None, oversized).unwrap();
        event.sign_with_keypair(&keypair);

        let result = event.validate();
        assert!(
            matches!(result, Err(EventValidationError::PayloadTooLarge { size, max }) if size == MAX_PAYLOAD_SIZE + 1 && max == MAX_PAYLOAD_SIZE),
            "Expected PayloadTooLarge, got {result:?}"
        );
    }

    /// Test that the CreatorPubkeyMismatch error variant has the correct display message.
    #[test]
    fn test_creator_pubkey_mismatch_error_variant() {
        let err = EventValidationError::CreatorPubkeyMismatch {
            claimed: "aa".to_string(),
            derived: "bb".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("aa"), "Error message should contain claimed value");
        assert!(msg.contains("bb"), "Error message should contain derived value");
    }

    /// Test that the PayloadTooLarge error variant has the correct display message.
    #[test]
    fn test_payload_too_large_error_variant() {
        let err = EventValidationError::PayloadTooLarge {
            size: 2_000_000,
            max: MAX_PAYLOAD_SIZE,
        };
        let msg = format!("{err}");
        assert!(msg.contains("2000000"), "Error message should contain size");
        assert!(
            msg.contains(&MAX_PAYLOAD_SIZE.to_string()),
            "Error message should contain max"
        );
    }
}
