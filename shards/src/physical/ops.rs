//! Physical shard operations
//!
//! Defines operations for anchoring real-world items, transferring ownership,
//! and verifying provenance chains.

use serde::{Deserialize, Serialize};

/// Unique identifier for a physical item.
pub type ItemId = [u8; 32];

/// Owner identifier (public key).
pub type OwnerId = [u8; 32];

/// Operations supported by the Physical shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PhysicalOp {
    /// Anchor a new item on-chain with an initial owner.
    AnchorItem {
        /// Unique item identifier.
        item_id: ItemId,
        /// Initial owner.
        owner: OwnerId,
        /// Item metadata (e.g., description, serial number hash).
        metadata: Vec<u8>,
    },
    /// Transfer ownership of an item.
    TransferOwnership {
        /// The item to transfer.
        item_id: ItemId,
        /// The new owner.
        new_owner: OwnerId,
    },
    /// Verify the full provenance chain of an item.
    VerifyChain {
        /// The item whose chain should be verified.
        item_id: ItemId,
    },
}
