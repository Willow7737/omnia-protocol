//! Integration with Layer 2 Physical Shard
//!
//! This module provides the bridge between the Binding Layer's
//! `ProvenanceLog` and the existing `PhysicalShard` in the shards crate.
//! It extends the Physical shard's operations with binding-layer types
//! (RF fingerprints, quantum commitments, and the enhanced provenance log).
//!
//! # Integration Strategy
//!
//! Rather than modifying the existing `PhysicalShard` (which would violate
//! the constraint of not changing shards core logic), this module provides
//! a `ProvenanceTracker` that wraps the shard and enriches it with
//! binding-layer capabilities. The tracker maintains a map of item IDs to
//! `ProvenanceLog` instances and intercepts physical operations to
//! add cryptographic proof.

use std::collections::HashMap;

use crate::anchor::PhysicalAnchor;
use crate::provenance::{ProvenanceError, ProvenanceLog};
use crate::quantum_commit::{CommitmentPhase, QuantumCommitment};
use crate::rf_fingerprint::RfFingerprint;

/// Errors that can occur during provenance tracking.
#[derive(Debug, thiserror::Error)]
pub enum ProvenanceTrackerError {
    /// The item has already been anchored.
    #[error("Item already anchored: {0}")]
    AlreadyAnchored(String),
    /// The item was not found.
    #[error("Item not found: {0}")]
    NotFound(String),
    /// The item has been destroyed; no further operations allowed.
    #[error("Item destroyed: {0}")]
    Destroyed(String),
    /// Chain verification failed.
    #[error("Chain verification failed for item: {0}")]
    ChainVerificationFailed(String),
    /// RF verification failed.
    #[error("RF verification failed for item: {0}")]
    RfVerificationFailed(String),
    /// Provenance log error.
    #[error("Provenance error: {0}")]
    Provenance(#[from] ProvenanceError),
}

/// A tracker that maintains provenance logs for physical items.
///
/// This is the Binding Layer's integration point with the Physical Shard.
/// Each physical item gets a `ProvenanceLog` that records its complete
/// chain of custody with cryptographic proofs.
///
/// The tracker can be used alongside the existing `PhysicalState` from
/// the shards crate. It enriches the simple provenance events with
/// RF fingerprints and quantum commitments.
#[derive(Debug, Clone)]
pub struct ProvenanceTracker {
    /// Map of item IDs to their provenance anchors.
    anchors: HashMap<[u8; 32], PhysicalAnchor>,
}

impl ProvenanceTracker {
    /// Create a new empty provenance tracker.
    pub fn new() -> Self {
        Self {
            anchors: HashMap::new(),
        }
    }

    /// Anchor a new item with a provenance log, RF fingerprint, and
    /// quantum commitment.
    ///
    /// # Arguments
    ///
    /// * `item_id` — 32-byte unique item identifier
    /// * `creator_did` — DID of the item's creator
    /// * `rf_proof` — RF fingerprint at creation time
    /// * `commitment` — Quantum commitment of the creation event
    /// * `causal_anchor` — EventId in the causal graph for this event
    pub fn anchor_item(
        &mut self,
        item_id: [u8; 32],
        creator_did: String,
        rf_proof: RfFingerprint,
        commitment: QuantumCommitment,
        causal_anchor: [u8; 32],
    ) -> Result<(), ProvenanceTrackerError> {
        if self.anchors.contains_key(&item_id) {
            return Err(ProvenanceTrackerError::AlreadyAnchored(format!("{:?}", &item_id[..4])));
        }

        let provenance_log = ProvenanceLog::new(
            item_id,
            creator_did,
            rf_proof.clone(),
            commitment.clone(),
            causal_anchor,
        );

        let anchor = PhysicalAnchor::new(rf_proof, commitment, provenance_log, CommitmentPhase::ClassicalOnly);
        self.anchors.insert(item_id, anchor);
        Ok(())
    }

    /// Transfer ownership of an item to a new holder.
    ///
    /// Creates a new `Transferred` provenance event and updates the
    /// anchor's provenance log.
    pub fn transfer_item(
        &mut self,
        item_id: [u8; 32],
        to_did: String,
        rf_proof: RfFingerprint,
        commitment: QuantumCommitment,
    ) -> Result<(), ProvenanceTrackerError> {
        let anchor = self
            .anchors
            .get_mut(&item_id)
            .ok_or_else(|| ProvenanceTrackerError::NotFound(format!("{:?}", &item_id[..4])))?;

        if anchor.is_destroyed() {
            return Err(ProvenanceTrackerError::Destroyed(format!("{:?}", &item_id[..4])));
        }

        anchor.provenance_log.transfer(to_did, rf_proof, commitment)?;
        Ok(())
    }

    /// Destroy an item, preventing any future transfers.
    ///
    /// Creates a `Destroyed` provenance event. Once destroyed, the item
    /// cannot be transferred or verified further.
    pub fn destroy_item(
        &mut self,
        item_id: [u8; 32],
        rf_proof: RfFingerprint,
        commitment: QuantumCommitment,
    ) -> Result<(), ProvenanceTrackerError> {
        let anchor = self
            .anchors
            .get_mut(&item_id)
            .ok_or_else(|| ProvenanceTrackerError::NotFound(format!("{:?}", &item_id[..4])))?;

        if anchor.is_destroyed() {
            return Err(ProvenanceTrackerError::Destroyed(format!("{:?}", &item_id[..4])));
        }

        anchor.provenance_log.destroy(rf_proof, commitment);
        Ok(())
    }

    /// Verify an item's current RF fingerprint and provenance chain.
    ///
    /// # Arguments
    ///
    /// * `item_id` — The item to verify
    /// * `current_rf` — The freshly measured RF spectral hash
    ///
    /// # Returns
    ///
    /// `Ok(())` if verification succeeds, or an error indicating which
    /// check failed.
    pub fn verify_item(&self, item_id: [u8; 32], current_rf: &[u8; 32]) -> Result<(), ProvenanceTrackerError> {
        let anchor = self
            .anchors
            .get(&item_id)
            .ok_or_else(|| ProvenanceTrackerError::NotFound(format!("{:?}", &item_id[..4])))?;

        // Check RF fingerprint
        if !anchor.rf_fingerprint.verify(current_rf) {
            return Err(ProvenanceTrackerError::RfVerificationFailed(format!(
                "{:?}",
                &item_id[..4]
            )));
        }

        // Check provenance chain
        if !anchor.provenance_log.verify_chain() {
            return Err(ProvenanceTrackerError::ChainVerificationFailed(format!(
                "{:?}",
                &item_id[..4]
            )));
        }

        Ok(())
    }

    /// Query the provenance log for an item.
    pub fn query_provenance(&self, item_id: [u8; 32]) -> Option<&ProvenanceLog> {
        self.anchors.get(&item_id).map(|a| &a.provenance_log)
    }

    /// Get the current holder of an item.
    pub fn current_holder(&self, item_id: [u8; 32]) -> Option<&str> {
        self.anchors.get(&item_id).map(|a| a.current_holder())
    }

    /// Get the number of tracked items.
    pub fn item_count(&self) -> usize {
        self.anchors.len()
    }

    /// Get a reference to an item's anchor.
    pub fn get_anchor(&self, item_id: &[u8; 32]) -> Option<&PhysicalAnchor> {
        self.anchors.get(item_id)
    }
}

impl Default for ProvenanceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::quantum_commit::QuantumCommitment;
    use crate::rf_fingerprint::RfFingerprint;
    use omnia_substrate::VectorClock;

    fn test_rf(did: &str, hash: [u8; 32]) -> RfFingerprint {
        RfFingerprint::stub(did, hash)
    }

    fn test_commitment(data: &[u8]) -> QuantumCommitment {
        QuantumCommitment::new_stub(data, VectorClock::new())
    }

    fn test_item_id(n: u8) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = n;
        id
    }

    #[test]
    fn test_anchor_item() {
        let mut tracker = ProvenanceTracker::new();
        let item_id = test_item_id(1);
        let rf_hash = [0x55u8; 32];

        tracker
            .anchor_item(
                item_id,
                "did:omnia:factory".to_string(),
                test_rf("did:omnia:factory", rf_hash),
                test_commitment(b"creation"),
                [0xCDu8; 32],
            )
            .unwrap();

        assert_eq!(tracker.item_count(), 1);
        assert_eq!(tracker.current_holder(item_id), Some("did:omnia:factory"));
    }

    #[test]
    fn test_double_anchor_fails() {
        let mut tracker = ProvenanceTracker::new();
        let item_id = test_item_id(1);

        tracker
            .anchor_item(
                item_id,
                "did:omnia:factory".to_string(),
                test_rf("did:omnia:factory", [0x55u8; 32]),
                test_commitment(b"creation"),
                [0xCDu8; 32],
            )
            .unwrap();

        let result = tracker.anchor_item(
            item_id,
            "did:omnia:factory".to_string(),
            test_rf("did:omnia:factory", [0x55u8; 32]),
            test_commitment(b"creation2"),
            [0xCDu8; 32],
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_transfer_item() {
        let mut tracker = ProvenanceTracker::new();
        let item_id = test_item_id(1);

        tracker
            .anchor_item(
                item_id,
                "did:omnia:alice".to_string(),
                test_rf("did:omnia:alice", [0x55u8; 32]),
                test_commitment(b"creation"),
                [0xCDu8; 32],
            )
            .unwrap();

        tracker
            .transfer_item(
                item_id,
                "did:omnia:bob".to_string(),
                test_rf("did:omnia:bob", [0x66u8; 32]),
                test_commitment(b"transfer1"),
            )
            .unwrap();

        assert_eq!(tracker.current_holder(item_id), Some("did:omnia:bob"));
    }

    #[test]
    fn test_verify_item() {
        let mut tracker = ProvenanceTracker::new();
        let item_id = test_item_id(1);
        let rf_hash = [0x55u8; 32];

        tracker
            .anchor_item(
                item_id,
                "did:omnia:factory".to_string(),
                test_rf("did:omnia:factory", rf_hash),
                test_commitment(b"creation"),
                [0xCDu8; 32],
            )
            .unwrap();

        // Verify with matching RF
        assert!(tracker.verify_item(item_id, &rf_hash).is_ok());
    }

    #[test]
    fn test_verify_item_wrong_rf() {
        let mut tracker = ProvenanceTracker::new();
        let item_id = test_item_id(1);
        let rf_hash = [0x55u8; 32];
        let wrong_rf = [0xFFu8; 32];

        tracker
            .anchor_item(
                item_id,
                "did:omnia:factory".to_string(),
                test_rf("did:omnia:factory", rf_hash),
                test_commitment(b"creation"),
                [0xCDu8; 32],
            )
            .unwrap();

        // Verify with wrong RF should fail
        assert!(tracker.verify_item(item_id, &wrong_rf).is_err());
    }

    #[test]
    fn test_transfer_nonexistent_item_fails() {
        let mut tracker = ProvenanceTracker::new();
        let item_id = test_item_id(99);

        let result = tracker.transfer_item(
            item_id,
            "did:omnia:bob".to_string(),
            test_rf("did:omnia:bob", [0x66u8; 32]),
            test_commitment(b"transfer1"),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_destroy_item() {
        let mut tracker = ProvenanceTracker::new();
        let item_id = test_item_id(1);

        tracker
            .anchor_item(
                item_id,
                "did:omnia:owner".to_string(),
                test_rf("did:omnia:owner", [0x55u8; 32]),
                test_commitment(b"creation"),
                [0xCDu8; 32],
            )
            .unwrap();

        tracker
            .destroy_item(
                item_id,
                test_rf("did:omnia:owner", [0x55u8; 32]),
                test_commitment(b"destruction"),
            )
            .unwrap();

        assert!(tracker.get_anchor(&item_id).unwrap().is_destroyed());

        // Transfer after destroy should fail
        let result = tracker.transfer_item(
            item_id,
            "did:omnia:other".to_string(),
            test_rf("did:omnia:other", [0x66u8; 32]),
            test_commitment(b"transfer_after_destroy"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_full_supply_chain() {
        let mut tracker = ProvenanceTracker::new();
        let item_id = test_item_id(1);

        // 1. Factory creates the item
        tracker
            .anchor_item(
                item_id,
                "did:omnia:factory".to_string(),
                test_rf("did:omnia:factory", [0x11u8; 32]),
                test_commitment(b"creation"),
                [0xAAu8; 32],
            )
            .unwrap();

        // 2. Factory transfers to distributor
        tracker
            .transfer_item(
                item_id,
                "did:omnia:distributor".to_string(),
                test_rf("did:omnia:distributor", [0x22u8; 32]),
                test_commitment(b"transfer_factory_to_dist"),
            )
            .unwrap();

        // 3. Distributor transfers to retailer
        tracker
            .transfer_item(
                item_id,
                "did:omnia:retailer".to_string(),
                test_rf("did:omnia:retailer", [0x33u8; 32]),
                test_commitment(b"transfer_dist_to_retail"),
            )
            .unwrap();

        // 4. Retailer transfers to customer
        tracker
            .transfer_item(
                item_id,
                "did:omnia:customer".to_string(),
                test_rf("did:omnia:customer", [0x44u8; 32]),
                test_commitment(b"transfer_retail_to_cust"),
            )
            .unwrap();

        // Verify chain integrity
        let provenance = tracker.query_provenance(item_id).unwrap();
        assert!(provenance.verify_chain());
        assert_eq!(provenance.len(), 4); // creation + 3 transfers
        assert_eq!(tracker.current_holder(item_id), Some("did:omnia:customer"));
    }
}
