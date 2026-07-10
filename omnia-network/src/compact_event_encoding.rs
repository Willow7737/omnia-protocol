//! Compact event encoding for gossip optimization
//!
//! Two complementary mechanisms live here:
//!
//! 1. **Broadcast wire format (integrated)** — [`encode_compact_wire`] /
//!    [`decode_compact_wire`] serialize events for the gossip topic with
//!    every *derivable* field elided: the 32-byte event ID (content hash),
//!    the 32-byte creator (hash of the public key), and the sender-claimed
//!    consensus status. The receiver recomputes them via
//!    `Event::from_signed_parts`, saving 64+ bytes per event AND removing
//!    the sender's ability to even express a mismatching ID or creator.
//!    Messages carry wire-format version byte `2`
//!    (`WIRE_FORMAT_VERSION_COMPACT_EVENT`); receivers accept both this
//!    and the full version-1 format.
//!
//! 2. **Per-peer delta compression (reserved for the sync path)** —
//!    [`CompactEncoder`] delta-encodes vector clocks against a per-peer
//!    frontier. This needs point-to-point context (each peer has a
//!    different frontier), which gossipsub topic broadcast cannot provide;
//!    it is intended for the fast-sync / event-request path where the
//!    remote frontier is known. Note that since the bincode→postcard wire
//!    migration, clock *values* are already varint-encoded on the wire —
//!    the remaining delta win is dropping unchanged entries (32-byte node
//!    IDs), which only a real frontier enables.

use omnia_primitives::{
    Event, EventId, NodeId, VectorClock, MAX_POSTCARD_INPUT_SIZE, WIRE_FORMAT_VERSION_COMPACT_EVENT,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Maximum size for a delta clock in bytes.
pub const DEFAULT_MAX_DELTA_CLOCK_SIZE: usize = 1024;

/// Maximum number of entries in a delta clock (prevents untrusted allocation).
const MAX_DELTA_CLOCK_ENTRIES: usize = 10_000;

/// Maximum encoded size for delta clock data (1 MiB).
const MAX_DELTA_CLOCK_ENCODED_SIZE: usize = 1_000_000;

/// Default truncation bytes for event IDs.
pub const DEFAULT_ID_TRUNCATION_BYTES: usize = 16;

/// Errors from compact event encoding/decoding.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EncodingError {
    /// Serialization failed.
    #[error("Serialization error: {0}")]
    SerializationError(String),
    /// Deserialization failed.
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    /// Delta clock exceeds maximum size.
    #[error("Delta clock too large: {size} bytes (max {max})")]
    DeltaClockTooLarge {
        /// Actual size in bytes.
        size: usize,
        /// Maximum allowed size in bytes.
        max: usize,
    },
    /// Frontier not found for the given peer.
    #[error("Frontier not found for peer")]
    FrontierNotFound,
    /// Invalid truncated event ID.
    #[error("Invalid truncated event ID: expected {expected} bytes, got {actual}")]
    InvalidTruncatedId {
        /// Expected number of bytes.
        expected: usize,
        /// Actual number of bytes received.
        actual: usize,
    },
    /// Delta clock application failed.
    #[error("Delta clock application failed: {0}")]
    DeltaApplicationFailed(String),
    /// Invalid format (e.g., untrusted count exceeds maximum).
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

/// Delta-encoded vector clock — only entries where local > remote.
///
/// Each entry is a `(NodeId, u64)` pair representing a counter that has
/// advanced beyond what the remote peer last saw. The entries are encoded
/// with varint compression for the clock values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaClock {
    /// Only the entries that changed (node_id -> new clock value).
    pub entries: Vec<(NodeId, u64)>,
}

impl DeltaClock {
    /// Create an empty delta clock.
    pub fn empty() -> Self {
        Self { entries: Vec::new() }
    }

    /// Check if the delta clock is empty (no entries changed).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries in the delta clock.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Encode the delta clock to bytes using varint encoding.
    ///
    /// Format: `[count:varint][(node_id:32bytes, clock_value:varint)...]`
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + self.entries.len() * 40);
        encode_varint(self.entries.len() as u64, &mut bytes);
        for (node_id, clock) in &self.entries {
            bytes.extend_from_slice(node_id);
            encode_varint(*clock, &mut bytes);
        }
        bytes
    }

    /// Decode a delta clock from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, EncodingError> {
        if data.len() > MAX_DELTA_CLOCK_ENCODED_SIZE {
            return Err(EncodingError::InvalidFormat("Delta clock data too large".to_string()));
        }
        let mut offset = 0usize;
        let count = decode_varint(data, &mut offset)? as usize;
        if count > MAX_DELTA_CLOCK_ENTRIES {
            return Err(EncodingError::InvalidFormat(format!(
                "Delta clock entry count {} exceeds maximum {}",
                count, MAX_DELTA_CLOCK_ENTRIES
            )));
        }
        // Check remaining data is sufficient for count entries
        // Minimum per entry: 32 bytes NodeId + 1 byte minimum varint
        let remaining = data.len().saturating_sub(offset);
        let required = count.saturating_mul(33); // NodeId(32) + min varint(1)
        if remaining < required {
            return Err(EncodingError::InvalidFormat(
                "Insufficient data for delta clock entries".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if offset + 32 > data.len() {
                return Err(EncodingError::DeserializationError(
                    "truncated node_id in delta clock".to_string(),
                ));
            }
            let mut node_id = [0u8; 32];
            node_id.copy_from_slice(&data[offset..offset + 32]);
            offset += 32;
            let clock = decode_varint(data, &mut offset)?;
            entries.push((node_id, clock));
        }
        Ok(Self { entries })
    }
}

/// Encode a varint into the output buffer.
fn encode_varint(value: u64, output: &mut Vec<u8>) {
    let mut v = value;
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            output.push(byte);
            break;
        }
        output.push(byte | 0x80);
    }
}

/// Decode a varint from the input buffer, advancing the offset.
fn decode_varint(data: &[u8], offset: &mut usize) -> Result<u64, EncodingError> {
    let mut result = 0u64;
    let mut shift = 0u32;
    loop {
        if *offset >= data.len() {
            return Err(EncodingError::DeserializationError(
                "unexpected end of varint".to_string(),
            ));
        }
        let byte = data[*offset];
        *offset += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err(EncodingError::DeserializationError("varint overflow".to_string()));
        }
    }
    Ok(result)
}

/// Compact wire representation of an Event.
///
/// Compared to the full `Event`, this representation:
/// - Uses a delta-encoded vector clock instead of the full clock
/// - Truncates event IDs (self_parent, other_parent) when they refer
///   to recent events from the sender
/// - Carries the payload as-is (snappy compression is applied at the
///   outer layer, not here)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactEvent {
    /// Full event ID (32 bytes — needed for dedup and graph lookup).
    pub id: EventId,
    /// Creator node ID.
    pub creator: NodeId,
    /// Sequence number from the creator.
    pub sequence: u64,
    /// Wall-clock timestamp.
    pub timestamp: u64,
    /// Delta-encoded vector clock against the remote frontier.
    pub delta_clock: DeltaClock,
    /// Truncated self-parent (first N bytes), or None for genesis.
    pub self_parent_truncated: Option<Vec<u8>>,
    /// Truncated other-parent (first N bytes), or None.
    pub other_parent_truncated: Option<Vec<u8>>,
    /// Application-specific payload.
    pub payload: Vec<u8>,
    /// Ed25519 public key of the creator (32 bytes).
    pub creator_pubkey: [u8; 32],
    /// Ed25519 signature over the event hash (64 bytes).
    #[serde(with = "serde_array_64")]
    pub signature: [u8; 64],
}

/// Compact encoder with per-peer frontier tracking.
///
/// The encoder maintains a map of the last-known vector clock frontier
/// for each remote peer. When encoding an event, it delta-compresses
/// the event's vector clock against the peer's frontier, sending only
/// the entries that have advanced.
///
/// On the decode side, the receiver applies the delta to its local
/// frontier for the sending peer to reconstruct the full vector clock.
#[derive(Clone, Debug)]
pub struct CompactEncoder {
    /// Last-known vector clock frontier per peer (NodeId -> VectorClock).
    peer_frontiers: HashMap<NodeId, VectorClock>,
    /// Maximum delta clock size in bytes.
    pub max_delta_clock_size: usize,
    /// Number of bytes to truncate event IDs to.
    pub id_truncation_bytes: usize,
}

impl Default for CompactEncoder {
    fn default() -> Self {
        Self {
            peer_frontiers: HashMap::new(),
            max_delta_clock_size: DEFAULT_MAX_DELTA_CLOCK_SIZE,
            id_truncation_bytes: DEFAULT_ID_TRUNCATION_BYTES,
        }
    }
}

impl CompactEncoder {
    /// Create a new compact encoder with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `max_delta_clock_size` — Maximum size of a delta clock in bytes.
    /// * `id_truncation_bytes` — Number of bytes to truncate event IDs to.
    pub fn new(max_delta_clock_size: usize, id_truncation_bytes: usize) -> Self {
        Self {
            peer_frontiers: HashMap::new(),
            max_delta_clock_size,
            id_truncation_bytes,
        }
    }

    /// Update the frontier for a peer.
    ///
    /// Call this when the peer's latest vector clock is learned
    /// (e.g., from a received gossip digest or sync response).
    pub fn update_frontier(&mut self, peer_id: NodeId, frontier: VectorClock) {
        self.peer_frontiers.insert(peer_id, frontier);
    }

    /// Get the frontier for a peer, if known.
    pub fn get_frontier(&self, peer_id: &NodeId) -> Option<&VectorClock> {
        self.peer_frontiers.get(peer_id)
    }

    /// Encode an event for transmission to a specific peer.
    ///
    /// The event's vector clock is delta-encoded against the peer's
    /// last-known frontier. If no frontier is known for the peer,
    /// the full vector clock is sent as a delta from an empty clock.
    ///
    /// # Arguments
    ///
    /// * `event` — The event to encode.
    /// * `peer_id` — The ID of the peer we are sending to.
    ///
    /// # Errors
    ///
    /// Returns [`EncodingError::DeltaClockTooLarge`] if the delta clock
    /// exceeds the configured maximum size.
    pub fn encode(&self, event: &Event, peer_id: &NodeId) -> Result<CompactEvent, EncodingError> {
        let remote_frontier = self.peer_frontiers.get(peer_id).cloned().unwrap_or_default();
        let delta_clock = Self::encode_delta_clock(&event.vector_clock, &remote_frontier);

        // Check delta clock size
        let delta_bytes = delta_clock.to_bytes();
        if delta_bytes.len() > self.max_delta_clock_size {
            return Err(EncodingError::DeltaClockTooLarge {
                size: delta_bytes.len(),
                max: self.max_delta_clock_size,
            });
        }

        let truncation = self.id_truncation_bytes.min(32);

        let self_parent_truncated = event.self_parent.map(|id| id[..truncation].to_vec());
        let other_parent_truncated = event.other_parent.map(|id| id[..truncation].to_vec());

        Ok(CompactEvent {
            id: event.id,
            creator: event.creator,
            sequence: event.sequence,
            timestamp: event.timestamp,
            delta_clock,
            self_parent_truncated,
            other_parent_truncated,
            payload: event.payload.clone(),
            creator_pubkey: event.creator_pubkey,
            signature: event.signature,
        })
    }

    /// Decode a compact event back into a full Event.
    ///
    /// The delta clock is applied to the local frontier for the sending
    /// peer to reconstruct the full vector clock. Truncated event IDs
    /// are expanded by matching against the local graph's known events.
    ///
    /// # Arguments
    ///
    /// * `compact` — The compact event to decode.
    /// * `sender_id` — The ID of the peer that sent the event.
    /// * `local_frontier` — Our current frontier for the sending peer.
    /// * `resolve_truncated_id` — A function that resolves a truncated
    ///   event ID (first N bytes) to the full 32-byte ID. Returns `None`
    ///   if the full ID cannot be determined.
    ///
    /// # Errors
    ///
    /// Returns [`EncodingError`] if decoding fails.
    pub fn decode<F>(
        &mut self,
        compact: &CompactEvent,
        sender_id: &NodeId,
        local_frontier: &VectorClock,
        resolve_truncated_id: F,
    ) -> Result<Event, EncodingError>
    where
        F: Fn(&[u8]) -> Option<EventId>,
    {
        // Reconstruct full vector clock from delta
        let vector_clock = Self::apply_delta_clock(local_frontier, &compact.delta_clock);

        // Update frontier for this peer
        self.update_frontier(*sender_id, vector_clock.clone());

        // Resolve truncated parent IDs
        let self_parent = match compact.self_parent_truncated.as_ref() {
            Some(truncated) => Some(
                resolve_truncated_id(truncated).ok_or(EncodingError::InvalidTruncatedId {
                    expected: 32,
                    actual: truncated.len(),
                })?,
            ),
            None => None,
        };

        let other_parent = match compact.other_parent_truncated.as_ref() {
            Some(truncated) => Some(
                resolve_truncated_id(truncated).ok_or(EncodingError::InvalidTruncatedId {
                    expected: 32,
                    actual: truncated.len(),
                })?,
            ),
            None => None,
        };

        Ok(Event {
            id: compact.id,
            creator: compact.creator,
            sequence: compact.sequence,
            timestamp: compact.timestamp,
            vector_clock,
            self_parent,
            other_parent,
            payload: compact.payload.clone(),
            creator_pubkey: compact.creator_pubkey,
            signature: compact.signature,
            status: omnia_primitives::EventStatus::Pending,
            ack_count: 0,
        })
    }

    /// Encode a delta clock: only entries where local > remote.
    pub fn encode_delta_clock(local: &VectorClock, remote: &VectorClock) -> DeltaClock {
        let mut entries = Vec::new();

        // Find all entries in local that are greater than remote
        for node_id in local.nodes() {
            let local_val = local.get(node_id);
            let remote_val = remote.get(node_id);
            if local_val > remote_val {
                entries.push((*node_id, local_val));
            }
        }

        DeltaClock { entries }
    }

    /// Apply a delta clock to a base vector clock.
    ///
    /// The result is `max(base, delta)` for each entry in the delta.
    /// Existing entries in the base that are not in the delta are preserved.
    pub fn apply_delta_clock(base: &VectorClock, delta: &DeltaClock) -> VectorClock {
        let mut result = base.clone();
        for (node_id, clock) in &delta.entries {
            let current = result.get(node_id);
            if *clock > current {
                result.set(*node_id, *clock);
            }
        }
        result
    }

    /// Serialize a compact event to bytes using postcard.
    pub fn serialize_compact(compact: &CompactEvent) -> Result<Vec<u8>, EncodingError> {
        postcard::to_allocvec(compact).map_err(|e| EncodingError::SerializationError(e.to_string()))
    }

    /// Deserialize a compact event from bytes using postcard.
    pub fn deserialize_compact(data: &[u8]) -> Result<CompactEvent, EncodingError> {
        postcard::from_bytes(data).map_err(|e| EncodingError::DeserializationError(e.to_string()))
    }

    /// Estimate the wire-size savings from compact encoding.
    ///
    /// Returns the estimated number of bytes saved compared to the
    /// full event serialization.
    pub fn estimate_savings(event: &Event, peer_frontier: &VectorClock) -> usize {
        let full_vc_bytes = event
            .vector_clock
            .to_bytes()
            .map_err(|e| e.to_string())
            .unwrap_or_default()
            .len();
        let delta = Self::encode_delta_clock(&event.vector_clock, peer_frontier);
        let delta_bytes = delta.to_bytes().len();

        // Savings from delta encoding of vector clock
        let vc_savings = full_vc_bytes.saturating_sub(delta_bytes);

        // Savings from truncating event IDs (assume 2 parents with 32-byte IDs)
        let parent_savings = if event.self_parent.is_some() {
            32 - DEFAULT_ID_TRUNCATION_BYTES
        } else {
            0
        } + if event.other_parent.is_some() {
            32 - DEFAULT_ID_TRUNCATION_BYTES
        } else {
            0
        };

        vc_savings + parent_savings
    }
}

// ── Peer-agnostic wire helpers (broadcast path) ─────────────────────────
//
// `CompactEncoder::encode`/`decode` delta-compress against a *per-peer*
// frontier, which fits point-to-point sync but not gossipsub broadcast
// (one published message reaches peers with different frontiers, and the
// postcard wire format already varint-encodes clock values). What broadcast
// CAN safely shed are the event's *derivable* fields: the 32-byte creator
// (a domain-separated hash of the public key) and the 32-byte event ID
// (the content hash), plus the sender-claimed consensus status. The
// receiver recomputes all of them via `Event::from_signed_parts`, so a
// sender cannot even express a mismatching ID or creator on the wire.

/// Compact broadcast representation: an [`Event`] minus every field the
/// receiver can (and must) derive itself — `id`, `creator`, `status`,
/// and `ack_count`. Saves 64+ bytes per event on the wire.
#[derive(Serialize, Deserialize)]
struct CompactWireEvent {
    sequence: u64,
    timestamp: u64,
    vector_clock: VectorClock,
    self_parent: Option<EventId>,
    other_parent: Option<EventId>,
    payload: Vec<u8>,
    creator_pubkey: [u8; 32],
    #[serde(with = "serde_array_64")]
    signature: [u8; 64],
}

/// Encode an event for topic broadcast: `[version 2] ++ postcard(CompactWireEvent)`.
///
/// Derivable fields (`id`, `creator`, consensus status) are omitted; the
/// receiver recomputes them from the signed content. The round trip is
/// lossless by construction — there is nothing to reconstruct that is not
/// carried verbatim or recomputed from it.
pub fn encode_compact_wire(event: &Event) -> Result<Vec<u8>, EncodingError> {
    let wire = CompactWireEvent {
        sequence: event.sequence,
        timestamp: event.timestamp,
        vector_clock: event.vector_clock.clone(),
        self_parent: event.self_parent,
        other_parent: event.other_parent,
        payload: event.payload.clone(),
        creator_pubkey: event.creator_pubkey,
        signature: event.signature,
    };
    let mut bytes = vec![WIRE_FORMAT_VERSION_COMPACT_EVENT];
    bytes.extend(postcard::to_allocvec(&wire).map_err(|e| EncodingError::SerializationError(e.to_string()))?);
    Ok(bytes)
}

/// Check whether a wire message carries a compact-encoded event.
pub fn is_compact_wire(data: &[u8]) -> bool {
    data.first() == Some(&WIRE_FORMAT_VERSION_COMPACT_EVENT)
}

/// Decode a compact wire message produced by [`encode_compact_wire`].
///
/// The creator identity and event ID are recomputed from the carried
/// content (see [`Event::from_signed_parts`]); the returned event has
/// `Pending` status and a zero ack count — a receiver never trusts a
/// sender's claimed consensus state. Callers MUST still run
/// `Event::validate()` on the result to check the signature against the
/// recomputed ID.
pub fn decode_compact_wire(data: &[u8]) -> Result<Event, EncodingError> {
    if !is_compact_wire(data) {
        return Err(EncodingError::InvalidFormat(
            "missing compact wire version byte".to_string(),
        ));
    }
    if data.len() > MAX_POSTCARD_INPUT_SIZE {
        return Err(EncodingError::InvalidFormat(
            "compact wire input exceeds size limit".to_string(),
        ));
    }
    let wire: CompactWireEvent =
        postcard::from_bytes(&data[1..]).map_err(|e| EncodingError::DeserializationError(e.to_string()))?;
    Event::from_signed_parts(
        wire.sequence,
        wire.timestamp,
        wire.vector_clock,
        wire.self_parent,
        wire.other_parent,
        wire.payload,
        wire.creator_pubkey,
        wire.signature,
    )
    .map_err(|e| EncodingError::DeserializationError(e.to_string()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    #[test]
    fn test_delta_clock_empty() {
        let delta = DeltaClock::empty();
        assert!(delta.is_empty());
        assert_eq!(delta.len(), 0);
    }

    #[test]
    fn test_delta_clock_roundtrip() {
        let delta = DeltaClock {
            entries: vec![(node(1), 42), (node(2), 100), (node(3), 1)],
        };
        let bytes = delta.to_bytes();
        let restored = DeltaClock::from_bytes(&bytes).unwrap();
        assert_eq!(delta, restored);
    }

    #[test]
    fn test_encode_delta_clock_empty_remote() {
        let local = VectorClock::with_node(node(1), 5);
        let remote = VectorClock::new();

        let delta = CompactEncoder::encode_delta_clock(&local, &remote);
        assert_eq!(delta.entries.len(), 1);
        assert_eq!(delta.entries[0], (node(1), 5));
    }

    #[test]
    fn test_encode_delta_clock_partial() {
        let mut local = VectorClock::new();
        local.set(node(1), 5);
        local.set(node(2), 3);
        local.set(node(3), 10);

        let mut remote = VectorClock::new();
        remote.set(node(1), 3);
        remote.set(node(2), 3);
        // node(3) not in remote

        let delta = CompactEncoder::encode_delta_clock(&local, &remote);
        // Only node(1) (5 > 3) and node(3) (10 > 0) should be in delta
        assert_eq!(delta.entries.len(), 2);
        assert!(delta.entries.contains(&(node(1), 5)));
        assert!(delta.entries.contains(&(node(3), 10)));
    }

    #[test]
    fn test_encode_delta_clock_no_change() {
        let mut local = VectorClock::new();
        local.set(node(1), 5);

        let mut remote = VectorClock::new();
        remote.set(node(1), 5);

        let delta = CompactEncoder::encode_delta_clock(&local, &remote);
        assert!(delta.is_empty());
    }

    #[test]
    fn test_apply_delta_clock() {
        let mut base = VectorClock::new();
        base.set(node(1), 3);
        base.set(node(2), 7);

        let delta = DeltaClock {
            entries: vec![
                (node(1), 5),  // advances node(1) from 3 to 5
                (node(3), 10), // adds node(3)
            ],
        };

        let result = CompactEncoder::apply_delta_clock(&base, &delta);
        assert_eq!(result.get(&node(1)), 5); // updated
        assert_eq!(result.get(&node(2)), 7); // unchanged
        assert_eq!(result.get(&node(3)), 10); // added
    }

    #[test]
    fn test_apply_delta_clock_no_regression() {
        let mut base = VectorClock::new();
        base.set(node(1), 10);

        let delta = DeltaClock {
            entries: vec![(node(1), 5)], // delta is less than base
        };

        let result = CompactEncoder::apply_delta_clock(&base, &delta);
        assert_eq!(result.get(&node(1)), 10); // should not regress
    }

    #[test]
    fn test_varint_encoding_roundtrip() {
        let values = [0, 1, 127, 128, 255, 256, 16383, 16384, u64::MAX];
        for &value in &values {
            let mut buf = Vec::new();
            encode_varint(value, &mut buf);
            let mut offset = 0usize;
            let decoded = decode_varint(&buf, &mut offset).unwrap();
            assert_eq!(decoded, value, "varint roundtrip failed for {value}");
        }
    }

    #[test]
    fn test_compact_encoder_default() {
        let encoder = CompactEncoder::default();
        assert_eq!(encoder.max_delta_clock_size, DEFAULT_MAX_DELTA_CLOCK_SIZE);
        assert_eq!(encoder.id_truncation_bytes, DEFAULT_ID_TRUNCATION_BYTES);
    }

    #[test]
    fn test_compact_encoder_encode_decode() {
        let mut encoder = CompactEncoder::new(1024, 16);

        let keypair = omnia_crypto::generate_keypair();
        let mut event = Event::genesis(node(1), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");

        let peer_id = node(2);
        let compact = encoder.encode(&event, &peer_id).unwrap();

        // The compact event should have a delta clock with at least one entry
        assert!(!compact.delta_clock.is_empty());

        // Decode
        let local_frontier = VectorClock::new();
        let decoded = encoder
            .decode(&compact, &peer_id, &local_frontier, |truncated| {
                // Simple resolver: check if it matches the event's self_parent
                if let Some(sp) = event.self_parent {
                    if sp[..16] == truncated[..] {
                        return Some(sp);
                    }
                }
                None
            })
            .unwrap();

        assert_eq!(decoded.id, event.id);
        assert_eq!(decoded.creator, event.creator);
        assert_eq!(decoded.sequence, event.sequence);
        assert_eq!(decoded.payload, event.payload);
    }

    #[test]
    fn test_compact_event_serialization_roundtrip() {
        let compact = CompactEvent {
            id: [1u8; 32],
            creator: [2u8; 32],
            sequence: 42,
            timestamp: 1234567890,
            delta_clock: DeltaClock {
                entries: vec![([3u8; 32], 100)],
            },
            self_parent_truncated: Some(vec![4u8; 16]),
            other_parent_truncated: None,
            payload: vec![5, 6, 7],
            creator_pubkey: [8u8; 32],
            signature: [9u8; 64],
        };

        let bytes = CompactEncoder::serialize_compact(&compact).unwrap();
        let restored = CompactEncoder::deserialize_compact(&bytes).unwrap();
        assert_eq!(compact, restored);
    }

    #[test]
    fn test_delta_clock_size_limit() {
        let encoder = CompactEncoder::new(10, 16); // Very small limit

        // Create an event with a large vector clock
        let mut vc = VectorClock::new();
        for i in 0..50u8 {
            vc.set(node(i), i as u64);
        }

        let keypair = omnia_crypto::generate_keypair();
        let mut event = Event::new(node(1), 0, vc, None, None, vec![]).expect("valid event");
        event.sign_with_keypair(&keypair).expect("signing");

        let peer_id = node(2);
        let result = encoder.encode(&event, &peer_id);
        // With 50 entries, delta clock will definitely exceed 10 bytes
        assert!(
            result.is_err(),
            "Expected DeltaClockTooLarge error for large vector clock with small limit"
        );
    }

    #[test]
    fn test_estimate_savings() {
        let mut local = VectorClock::new();
        local.set(node(1), 5);
        local.set(node(2), 3);

        let keypair = omnia_crypto::generate_keypair();
        let mut event = Event::genesis(node(1), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");

        let remote = VectorClock::new();
        let savings = CompactEncoder::estimate_savings(&event, &remote);
        // Should have some savings from delta encoding and ID truncation
        assert!(savings > 0, "Expected non-zero savings from compact encoding");
    }

    #[test]
    fn test_frontier_tracking() {
        let mut encoder = CompactEncoder::new(1024, 16);

        let peer_id = node(2);
        assert!(encoder.get_frontier(&peer_id).is_none());

        let frontier = VectorClock::with_node(node(2), 5);
        encoder.update_frontier(peer_id, frontier.clone());

        let stored = encoder.get_frontier(&peer_id).unwrap();
        assert_eq!(*stored, frontier);
    }

    // ── Peer-agnostic wire helper tests ─────────────────────────────────

    #[test]
    fn test_compact_wire_roundtrip_validates() {
        // A signed event must survive the wire round trip losslessly:
        // same ID, and Event::validate() (hash + signature) must pass on
        // the reconstructed event.
        let keypair = omnia_crypto::generate_keypair();
        let mut event = Event::genesis(node(7), vec![1, 2, 3, 4]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");

        let bytes = encode_compact_wire(&event).unwrap();
        assert!(is_compact_wire(&bytes));
        assert_eq!(bytes[0], WIRE_FORMAT_VERSION_COMPACT_EVENT);

        let decoded = decode_compact_wire(&bytes).unwrap();
        assert_eq!(decoded.id, event.id);
        assert_eq!(decoded.vector_clock, event.vector_clock);
        assert_eq!(decoded.self_parent, event.self_parent);
        assert_eq!(decoded.other_parent, event.other_parent);
        decoded
            .validate()
            .expect("reconstructed event must pass hash + signature validation");
    }

    #[test]
    fn test_compact_wire_roundtrip_with_parents() {
        let keypair = omnia_crypto::generate_keypair();
        let mut clock = VectorClock::new();
        clock.set(node(1), 3);
        clock.set(node(2), 7);
        let mut event =
            Event::new(node(1), 3, clock, Some([11u8; 32]), Some([22u8; 32]), vec![9, 9]).expect("valid event");
        event.sign_with_keypair(&keypair).expect("signing");

        let bytes = encode_compact_wire(&event).unwrap();
        let decoded = decode_compact_wire(&bytes).unwrap();
        assert_eq!(decoded.self_parent, Some([11u8; 32]));
        assert_eq!(decoded.other_parent, Some([22u8; 32]));
        decoded.validate().expect("reconstructed event must validate");
    }

    #[test]
    fn test_compact_wire_smaller_than_full_format() {
        // The compact format elides id (32B) + creator (32B) + consensus
        // status, so it must always beat the full format by ~64 bytes.
        let keypair = omnia_crypto::generate_keypair();
        let mut clock = VectorClock::new();
        for i in 0..10u8 {
            clock.set(node(i + 1), 1000 + i as u64);
        }
        let mut event = Event::new(node(1), 5, clock, Some([1u8; 32]), None, vec![0u8; 64]).expect("valid event");
        event.sign_with_keypair(&keypair).expect("signing");

        let full = event.to_bytes().expect("full serialization");
        let compact = encode_compact_wire(&event).unwrap();
        assert!(
            compact.len() + 60 <= full.len(),
            "compact ({}) should be at least 60 bytes smaller than full ({})",
            compact.len(),
            full.len()
        );
    }

    #[test]
    fn test_decode_compact_wire_rejects_wrong_version() {
        let data = [1u8, 0, 0, 0];
        assert!(!is_compact_wire(&data));
        assert!(decode_compact_wire(&data).is_err());
    }

    #[test]
    fn test_decode_compact_wire_rejects_garbage() {
        let data = [WIRE_FORMAT_VERSION_COMPACT_EVENT, 0xFF, 0xFF];
        assert!(decode_compact_wire(&data).is_err());
    }

    #[test]
    fn test_compact_wire_tamper_fails_validation() {
        // Tampering with the payload changes the recomputed event ID, so
        // the signature (made over the original ID) no longer verifies.
        let keypair = omnia_crypto::generate_keypair();
        let mut event = Event::genesis(node(3), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");

        let mut bytes = encode_compact_wire(&event).unwrap();
        let last = bytes.len() - 1; // flips a signature byte
        bytes[last] ^= 0xFF;

        // Decode may succeed (the bytes are structurally valid), but the
        // reconstructed event must fail validation.
        if let Ok(decoded) = decode_compact_wire(&bytes) {
            assert!(decoded.validate().is_err(), "tampered event must not pass validation");
        }
    }
}
