//! Physical shard state
//!
//! Maintains the provenance log for physical items. Each item has an
//! append-only list of provenance events that records its entire history.
//! This is naturally CRDT-friendly because appends are commutative.

use std::collections::HashMap;

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use super::ops::PhysicalOp;
use crate::shard::ShardError;

/// A single entry in an item's provenance log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceEvent {
    /// The type of event (anchor, transfer, etc.).
    pub event_type: String,
    /// The owner at this point in the chain.
    pub owner: super::ops::OwnerId,
    /// Vector clock when this event occurred.
    pub clock: VectorClock,
    /// Optional metadata.
    pub metadata: Vec<u8>,
}

/// The full state of the Physical shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalState {
    /// Provenance logs — maps item IDs to their ordered list of events.
    pub provenance: HashMap<super::ops::ItemId, Vec<ProvenanceEvent>>,
}

impl PhysicalState {
    /// Create an empty physical state.
    pub fn new() -> Self {
        Self {
            provenance: HashMap::new(),
        }
    }

    /// Apply a physical operation, mutating state.
    pub fn apply(&mut self, op: &PhysicalOp, vc: &VectorClock) -> Result<(), ShardError> {
        match op {
            PhysicalOp::AnchorItem {
                item_id,
                owner,
                metadata,
            } => {
                if self.provenance.contains_key(item_id) {
                    return Err(ShardError::StateConflict(format!(
                        "Item already anchored: {:?}",
                        item_id
                    )));
                }
                self.provenance.insert(
                    *item_id,
                    vec![ProvenanceEvent {
                        event_type: "anchor".into(),
                        owner: *owner,
                        clock: vc.clone(),
                        metadata: metadata.clone(),
                    }],
                );
                Ok(())
            }
            PhysicalOp::TransferOwnership { item_id, new_owner } => {
                let log = self
                    .provenance
                    .get_mut(item_id)
                    .ok_or_else(|| ShardError::ValidationFailed("Item not found".into()))?;

                log.push(ProvenanceEvent {
                    event_type: "transfer".into(),
                    owner: *new_owner,
                    clock: vc.clone(),
                    metadata: Vec::new(),
                });
                Ok(())
            }
            PhysicalOp::VerifyChain { item_id } => {
                if !self.provenance.contains_key(item_id) {
                    return Err(ShardError::ValidationFailed("Item not found".into()));
                }
                // Verification passes — in a real implementation, this would
                // check cryptographic proofs of each provenance event.
                Ok(())
            }
        }
    }

    /// Get the current owner of an item (the owner in the last provenance event).
    pub fn current_owner(&self, item_id: &super::ops::ItemId) -> Option<super::ops::OwnerId> {
        self.provenance
            .get(item_id)
            .and_then(|log| log.last())
            .map(|event| event.owner)
    }

    /// Serialize the state to bytes for snapshots.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("PhysicalState serialization cannot fail")
    }

    /// Deserialize state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

impl Default for PhysicalState {
    fn default() -> Self {
        Self::new()
    }
}
