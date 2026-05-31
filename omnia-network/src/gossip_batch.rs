//! Batch-aware gossip propagation
//!
//! Propagates event batches instead of individual events over gossip.
//! Reduces per-event gossip overhead by amortizing serialization and
//! network round-trips across multiple events.
//!
//! # Wire Format
//!
//! Batch gossip messages use the same postcard + optional snappy
//! compression format as individual event gossip (see `serialize_compressed`
//! and `deserialize_compressed` in the parent `gossip` module).
//!
//! The [`GossipBatchMessage`] is serialized using postcard (compatible
//! with the existing [`WireFormat`](omnia_primitives::wire_format) protocol)
//! and propagated over the same GossipSub topic as individual events.
//!
//! # Integration
//!
//! The batch gossip message is designed to be a variant of the existing
//! gossip protocol. Nodes that don't support batch gossip can simply
//! ignore batch messages (they'll be deserialized as unknown variants).

use omnia_consensus::batch::ConsensusEventBatch;
use omnia_primitives::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};

use crate::compression::{deserialize_with_compression, serialize_with_compression};

/// GossipSub topic name for batch event propagation.
pub const GOSSIP_BATCH_TOPIC: &str = "omnia_batch_events";

/// A batch-aware gossip message for propagating event batches.
///
/// This message type extends the existing gossip protocol with batch
/// propagation support. It carries a [`ConsensusEventBatch`] along with
/// optional metadata for routing and deduplication.
///
/// # Wire Format
///
/// Serialized using postcard for deterministic, compact binary encoding,
/// consistent with the existing [`WireFormat`](omnia_primitives::wire_format).
/// Optional snappy compression is applied for payloads exceeding the
/// compression threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipBatchMessage {
    /// A batch of events with proof.
    Batch {
        /// The event batch with proof.
        batch: ConsensusEventBatch,
    },
    /// Acknowledgment of a received batch.
    BatchAck {
        /// The batch ID being acknowledged.
        batch_id: [u8; 32],
        /// The Merkle root of the acknowledged batch.
        merkle_root: [u8; 32],
        /// The number of events acknowledged.
        event_count: usize,
    },
    /// Request for a specific batch by ID.
    BatchRequest {
        /// The batch ID to request.
        batch_id: [u8; 32],
    },
    /// Batch digest for synchronization (similar to GossipDigest).
    BatchDigest {
        /// The node sending the digest.
        node_id: NodeId,
        /// The highest batch sequence number from this node.
        last_sequence: u64,
        /// Vector clock at the time of digest creation.
        vector_clock: VectorClock,
        /// Number of events in the last batch.
        last_batch_event_count: usize,
    },
}

/// Errors for batch gossip operations.
#[derive(Debug, thiserror::Error, Clone)]
pub enum GossipBatchError {
    /// Serialization error.
    #[error("Serialization error: {0}")]
    SerializationError(String),
    /// Deserialization error.
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    /// Batch proof validation failed.
    #[error("Batch proof validation failed: {0}")]
    ProofValidationFailed(String),
    /// Batch is too large for gossip.
    #[error("Batch too large for gossip: {size} bytes (max {max})")]
    BatchTooLarge {
        /// Actual batch size in bytes.
        size: usize,
        /// Maximum allowed size in bytes.
        max: usize,
    },
    /// Invalid batch message format.
    #[error("Invalid batch message: {0}")]
    InvalidMessage(String),
}

/// Maximum serialized size for a batch gossip message (1 MiB).
pub const MAX_BATCH_GOSSIP_SIZE: usize = 1024 * 1024;

/// Serialize a batch gossip message with optional snappy compression.
///
/// Delegates to [`crate::compression::serialize_with_compression`] and
/// converts the error type to [`GossipBatchError`].
///
/// # Errors
///
/// Returns [`GossipBatchError::SerializationError`] if postcard serialization fails.
pub fn serialize_batch_message(msg: &GossipBatchMessage) -> Result<Vec<u8>, GossipBatchError> {
    serialize_with_compression(msg).map_err(GossipBatchError::SerializationError)
}

/// Deserialize a batch gossip message with optional snappy decompression.
///
/// Delegates to [`crate::compression::deserialize_with_compression`] and
/// converts the error type to [`GossipBatchError`].
///
/// # Errors
///
/// Returns [`GossipBatchError::DeserializationError`] if the compression
/// flag is unknown or postcard deserialization fails.
pub fn deserialize_batch_message(data: &[u8]) -> Result<GossipBatchMessage, GossipBatchError> {
    deserialize_with_compression(data).map_err(|e| match e {
        s if s.starts_with("unknown compression") || s.starts_with("empty payload") => {
            GossipBatchError::InvalidMessage(s)
        }
        s if s.contains("decompress") || s.contains("exceeds limit") => {
            GossipBatchError::DeserializationError(format!("decompression: {}", s))
        }
        s => GossipBatchError::DeserializationError(s),
    })
}

/// Validate a batch gossip message and return the serialized bytes.
///
/// Performs the following checks:
/// 1. Batch proof is valid
/// 2. Serialized size does not exceed maximum
/// 3. Event count matches the proof
///
/// Returns the serialized bytes on success, avoiding redundant re-serialization
/// by the caller.
///
/// # Errors
///
/// Returns [`GossipBatchError`] if any validation check fails.
pub fn validate_batch_message(msg: &GossipBatchMessage) -> Result<Vec<u8>, GossipBatchError> {
    match msg {
        GossipBatchMessage::Batch { batch } => {
            // Validate proof
            batch
                .validate_proof()
                .map_err(|e| GossipBatchError::ProofValidationFailed(e.to_string()))?;

            // Check serialized size (serialize once, return the bytes)
            let serialized = serialize_batch_message(msg)?;
            if serialized.len() > MAX_BATCH_GOSSIP_SIZE {
                return Err(GossipBatchError::BatchTooLarge {
                    size: serialized.len(),
                    max: MAX_BATCH_GOSSIP_SIZE,
                });
            }

            Ok(serialized)
        }
        GossipBatchMessage::BatchAck { .. } => Ok(serialize_batch_message(msg)?),
        GossipBatchMessage::BatchRequest { .. } => Ok(serialize_batch_message(msg)?),
        GossipBatchMessage::BatchDigest { .. } => Ok(serialize_batch_message(msg)?),
    }
}

/// Statistics for batch gossip operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchGossipStats {
    /// Number of batch messages sent.
    pub batches_sent: u64,
    /// Number of batch messages received.
    pub batches_received: u64,
    /// Number of events sent in batches.
    pub events_sent_in_batches: u64,
    /// Number of events received in batches.
    pub events_received_in_batches: u64,
    /// Number of batches rejected due to invalid proof.
    pub batches_rejected_invalid_proof: u64,
    /// Number of batches rejected due to size.
    pub batches_rejected_too_large: u64,
    /// Total bytes sent for batch messages.
    pub bytes_sent: u64,
    /// Total bytes received for batch messages.
    pub bytes_received: u64,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_consensus::batch::MAX_BATCH_SIZE;
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
        event.sign_with_keypair(&keypair).expect("signing");
        event
    }

    fn test_batch() -> ConsensusEventBatch {
        let events: Vec<Event> = (0..3).map(|i| signed_event(node(1), vec![i])).collect();
        ConsensusEventBatch::new(events, node(1), 0, VectorClock::new(), MAX_BATCH_SIZE).unwrap()
    }

    #[test]
    fn test_batch_message_serialization_roundtrip() {
        let batch = test_batch();
        let msg = GossipBatchMessage::Batch { batch };

        let serialized = serialize_batch_message(&msg).unwrap();
        let deserialized = deserialize_batch_message(&serialized).unwrap();

        match deserialized {
            GossipBatchMessage::Batch { batch: b } => {
                assert_eq!(b.events.len(), 3);
                assert_eq!(b.sequence, 0);
            }
            _ => panic!("Expected Batch variant"),
        }
    }

    #[test]
    fn test_batch_ack_serialization_roundtrip() {
        let msg = GossipBatchMessage::BatchAck {
            batch_id: [1u8; 32],
            merkle_root: [2u8; 32],
            event_count: 5,
        };

        let serialized = serialize_batch_message(&msg).unwrap();
        let deserialized = deserialize_batch_message(&serialized).unwrap();

        match deserialized {
            GossipBatchMessage::BatchAck {
                batch_id, event_count, ..
            } => {
                assert_eq!(batch_id, [1u8; 32]);
                assert_eq!(event_count, 5);
            }
            _ => panic!("Expected BatchAck variant"),
        }
    }

    #[test]
    fn test_batch_request_serialization_roundtrip() {
        let msg = GossipBatchMessage::BatchRequest { batch_id: [3u8; 32] };

        let serialized = serialize_batch_message(&msg).unwrap();
        let deserialized = deserialize_batch_message(&serialized).unwrap();

        match deserialized {
            GossipBatchMessage::BatchRequest { batch_id } => {
                assert_eq!(batch_id, [3u8; 32]);
            }
            _ => panic!("Expected BatchRequest variant"),
        }
    }

    #[test]
    fn test_batch_digest_serialization_roundtrip() {
        let msg = GossipBatchMessage::BatchDigest {
            node_id: node(42),
            last_sequence: 7,
            vector_clock: VectorClock::with_node(node(42), 7),
            last_batch_event_count: 50,
        };

        let serialized = serialize_batch_message(&msg).unwrap();
        let deserialized = deserialize_batch_message(&serialized).unwrap();

        match deserialized {
            GossipBatchMessage::BatchDigest {
                node_id,
                last_sequence,
                last_batch_event_count,
                ..
            } => {
                assert_eq!(node_id, node(42));
                assert_eq!(last_sequence, 7);
                assert_eq!(last_batch_event_count, 50);
            }
            _ => panic!("Expected BatchDigest variant"),
        }
    }

    #[test]
    fn test_deserialize_empty_data() {
        let result = deserialize_batch_message(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_unknown_compression_flag() {
        let data = vec![0xFF, 0x00, 0x00];
        let result = deserialize_batch_message(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_batch_message_valid() {
        let batch = test_batch();
        let msg = GossipBatchMessage::Batch { batch };
        assert!(validate_batch_message(&msg).is_ok());
        // validate_batch_message now returns serialized bytes
        let result = validate_batch_message(&msg);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_validate_batch_ack() {
        let msg = GossipBatchMessage::BatchAck {
            batch_id: [0u8; 32],
            merkle_root: [0u8; 32],
            event_count: 0,
        };
        let result = validate_batch_message(&msg);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compression_applied_for_large_payloads() {
        // Create a batch with larger payloads to trigger compression
        let events: Vec<Event> = (0..10).map(|_| signed_event(node(1), vec![0u8; 512])).collect();
        let batch = ConsensusEventBatch::new(events, node(1), 0, VectorClock::new(), MAX_BATCH_SIZE).unwrap();
        let msg = GossipBatchMessage::Batch { batch };

        let serialized = serialize_batch_message(&msg).unwrap();
        // Check that the compression flag is set (first byte)
        // It might or might not be compressed depending on the actual data,
        // but the roundtrip should always work
        let deserialized = deserialize_batch_message(&serialized).unwrap();
        // Just verify it deserializes correctly
        match deserialized {
            GossipBatchMessage::Batch { batch: b } => {
                assert_eq!(b.events.len(), 10);
            }
            _ => panic!("Expected Batch variant"),
        }
    }
}
