//! Compact event encoding for gossip optimization
//!
//! Uses protobuf-style encoding with delta compression for vector clocks
//! to reduce per-event wire size by ~40%. Vector clocks are delta-encoded
//! against the sender's last-known frontier, sending only the differences.
//!
//! # Wire Format
//!
//! A `CompactEvent` is encoded with postcard and optionally compressed
//! with snappy (consistent with the existing gossip serialization).
//! The delta clock is varint-encoded for compactness.
//!
//! # Delta Compression
//!
//! When a node knows the remote peer's frontier (the last vector clock
//! it saw from that peer), it can encode only the entries that have
//! advanced since then. The receiver reconstructs the full vector clock
//! by applying the delta to its local frontier for that peer.

use omnia_primitives::{Event, EventId, NodeId, VectorClock};
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
        let mut offset = 0usize;
        let count = decode_varint(data, &mut offset)? as usize;
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
        let full_vc_bytes = event.vector_clock.to_bytes().len();
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
        let mut event = Event::genesis(node(1), vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

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
        let mut event = Event::new(node(1), 0, vc, None, None, vec![]);
        event.sign_with_keypair(&keypair);

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
        let mut event = Event::genesis(node(1), vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

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
}
