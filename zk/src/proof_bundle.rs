//! Universal proof bundle format for multi-chain settlement.
//!
//! A [`ProofBundle`] encapsulates everything needed to verify an L2 state
//! transition on **any** L1 settlement layer: the state roots before and after,
//! the ZK proof, the batch Merkle root, and an L1 anchor for cross-chain
//! verification. The format is chain-agnostic — the same bundle can be
//! submitted to Ethereum, Bitcoin, Solana, or any future adapter.
//!
//! ## Serialization
//!
//! Bundles are serialized with [`bincode`] for compact, deterministic
//! encoding. This ensures that the same logical bundle always produces
//! the same byte sequence, which is critical for on-chain verification.

use serde::{Deserialize, Serialize};

/// Current format version for [`ProofBundle`].
pub const PROOF_BUNDLE_VERSION: u16 = 1;

/// EIP-155 chain ID for Ethereum mainnet.
const ETHEREUM_CHAIN_ID: u64 = 1;

/// Known Bitcoin chain IDs (mainnet and testnet variants).
const BITCOIN_CHAIN_IDS: &[u64] = &[0x80000000, 0x02000000, 0x01000000];

// ---------------------------------------------------------------------------
// L1Anchor
// ---------------------------------------------------------------------------

/// L1 chain reference for cross-chain proof anchoring.
///
/// Records the L1 block at which this proof was anchored, enabling
/// verification that the proof was submitted to a specific chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1Anchor {
    /// EIP-155 style chain ID (1 = Ethereum mainnet, etc.)
    pub chain_id: u64,
    /// L1 block height at time of anchoring.
    pub block_height: u64,
    /// L1 block timestamp (milliseconds since epoch).
    pub timestamp: u64,
}

impl L1Anchor {
    /// Create a new L1 anchor.
    pub fn new(chain_id: u64, block_height: u64, timestamp: u64) -> Self {
        Self {
            chain_id,
            block_height,
            timestamp,
        }
    }

    /// Returns `true` if this anchor refers to Ethereum mainnet (chain ID 1).
    pub fn is_ethereum(&self) -> bool {
        self.chain_id == ETHEREUM_CHAIN_ID
    }

    /// Returns `true` if this anchor refers to a known Bitcoin chain.
    ///
    /// Recognized Bitcoin chain IDs include mainnet, testnet, and signet
    /// variants as defined in BIP-44 / SLIP-44 coin types mapped to
    /// EIP-155-style identifiers.
    pub fn is_bitcoin(&self) -> bool {
        BITCOIN_CHAIN_IDS.contains(&self.chain_id)
    }
}

// ---------------------------------------------------------------------------
// ProofBundle
// ---------------------------------------------------------------------------

/// Chain-agnostic proof structure for multi-chain settlement.
///
/// A `ProofBundle` encapsulates everything needed to verify a state transition
/// on any L1 settlement layer: the state roots before and after, the ZK proof,
/// the batch Merkle root, and an L1 anchor for cross-chain verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofBundle {
    /// Format version for forward compatibility (currently 1).
    pub version: u16,
    /// BLAKE3 hash of the state after applying this batch.
    pub state_root: [u8; 32],
    /// BLAKE3 hash of the state before this batch.
    pub prev_state_root: [u8; 32],
    /// Serialized ZK proof bytes (R1CS proof in production, stub for Phase 0).
    pub transition_proof: Vec<u8>,
    /// BLAKE3 Merkle root of all events in this batch.
    pub batch_merkle_root: [u8; 32],
    /// L1 anchor data for cross-chain verification.
    pub l1_anchor: L1Anchor,
}

impl ProofBundle {
    /// Create a new proof bundle.
    ///
    /// The version field is automatically set to the current format version
    /// ([`PROOF_BUNDLE_VERSION`]).
    pub fn new(
        prev_state_root: [u8; 32],
        state_root: [u8; 32],
        transition_proof: Vec<u8>,
        batch_merkle_root: [u8; 32],
        l1_anchor: L1Anchor,
    ) -> Self {
        Self {
            version: PROOF_BUNDLE_VERSION,
            state_root,
            prev_state_root,
            transition_proof,
            batch_merkle_root,
            l1_anchor,
        }
    }

    /// Serialize the bundle to bytes using bincode.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProofBundleError> {
        bincode::serialize(self).map_err(|e| ProofBundleError::SerializationError(e.to_string()))
    }

    /// Deserialize a bundle from bytes using bincode.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProofBundleError> {
        bincode::deserialize(data).map_err(|e| ProofBundleError::SerializationError(e.to_string()))
    }

    /// Verify the integrity of this proof bundle.
    ///
    /// Checks that:
    /// - The format version is the current version (1)
    /// - The transition proof is non-empty
    /// - The previous and new state roots differ
    pub fn verify_integrity(&self) -> Result<(), ProofBundleError> {
        if self.version != PROOF_BUNDLE_VERSION {
            return Err(ProofBundleError::InvalidVersion(self.version));
        }

        if self.transition_proof.is_empty() {
            return Err(ProofBundleError::EmptyProof);
        }

        if self.state_root == self.prev_state_root {
            return Err(ProofBundleError::SameStateRoots);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ProofBundleError
// ---------------------------------------------------------------------------

/// Errors that can occur during proof bundle operations.
#[derive(Debug, thiserror::Error)]
pub enum ProofBundleError {
    /// The bundle format version is not supported.
    #[error("unsupported proof bundle version: {0}")]
    InvalidVersion(u16),
    /// The transition proof is empty.
    #[error("transition proof is empty")]
    EmptyProof,
    /// The previous and new state roots are identical.
    #[error("prev_state_root and state_root are identical")]
    SameStateRoots,
    /// Serialization or deserialization failure.
    #[error("serialization error: {0}")]
    SerializationError(String),
    /// General integrity check failure.
    #[error("integrity error: {0}")]
    IntegrityError(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a valid L1 anchor for Ethereum mainnet.
    fn eth_anchor() -> L1Anchor {
        L1Anchor::new(1, 19_000_000, 1_700_000_000_000)
    }

    /// Helper: create a valid proof bundle.
    fn valid_bundle() -> ProofBundle {
        let prev = [0u8; 32];
        let mut new_root = [0u8; 32];
        new_root[0] = 1;
        ProofBundle::new(
            prev,
            new_root,
            vec![0xAB; 192], // Phase 0 dummy proof
            [2u8; 32],
            eth_anchor(),
        )
    }

    #[test]
    fn test_valid_bundle_passes_integrity() {
        let bundle = valid_bundle();
        assert!(bundle.verify_integrity().is_ok());
    }

    #[test]
    fn test_reject_invalid_version() {
        let mut bundle = valid_bundle();
        bundle.version = 99;
        let err = bundle.verify_integrity().unwrap_err();
        assert!(
            matches!(err, ProofBundleError::InvalidVersion(99)),
            "expected InvalidVersion(99), got {:?}",
            err
        );
    }

    #[test]
    fn test_reject_empty_proof() {
        let mut bundle = valid_bundle();
        bundle.transition_proof = vec![];
        let err = bundle.verify_integrity().unwrap_err();
        assert!(
            matches!(err, ProofBundleError::EmptyProof),
            "expected EmptyProof, got {:?}",
            err
        );
    }

    #[test]
    fn test_reject_same_state_roots() {
        let root = [42u8; 32];
        let mut bundle = valid_bundle();
        bundle.prev_state_root = root;
        bundle.state_root = root;
        let err = bundle.verify_integrity().unwrap_err();
        assert!(
            matches!(err, ProofBundleError::SameStateRoots),
            "expected SameStateRoots, got {:?}",
            err
        );
    }

    #[test]
    fn test_serialization_roundtrip() {
        let bundle = valid_bundle();
        let bytes = bundle.to_bytes().expect("serialization should succeed");
        assert!(!bytes.is_empty());

        let restored = ProofBundle::from_bytes(&bytes).expect("deserialization should succeed");
        assert_eq!(restored.version, bundle.version);
        assert_eq!(restored.state_root, bundle.state_root);
        assert_eq!(restored.prev_state_root, bundle.prev_state_root);
        assert_eq!(restored.transition_proof, bundle.transition_proof);
        assert_eq!(restored.batch_merkle_root, bundle.batch_merkle_root);
        assert_eq!(restored.l1_anchor.chain_id, bundle.l1_anchor.chain_id);
        assert_eq!(
            restored.l1_anchor.block_height,
            bundle.l1_anchor.block_height
        );
        assert_eq!(restored.l1_anchor.timestamp, bundle.l1_anchor.timestamp);
    }

    #[test]
    fn test_deserialization_garbage_fails() {
        let result = ProofBundle::from_bytes(b"not a valid bundle");
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ProofBundleError::SerializationError(_)),
            "expected SerializationError"
        );
    }

    #[test]
    fn test_l1_anchor_ethereum_detection() {
        let anchor = L1Anchor::new(1, 100, 1000);
        assert!(anchor.is_ethereum());
        assert!(!anchor.is_bitcoin());

        let anchor_other = L1Anchor::new(137, 100, 1000); // Polygon
        assert!(!anchor_other.is_ethereum());
        assert!(!anchor_other.is_bitcoin());
    }

    #[test]
    fn test_l1_anchor_bitcoin_detection() {
        // Bitcoin mainnet (SLIP-44 coin type 0 mapped to EIP-155 style)
        let anchor = L1Anchor::new(0x80000000, 800_000, 1_700_000_000_000);
        assert!(anchor.is_bitcoin());
        assert!(!anchor.is_ethereum());

        // Non-bitcoin chain
        let anchor_other = L1Anchor::new(2, 100, 1000); // Exodus
        assert!(!anchor_other.is_bitcoin());
    }

    #[test]
    fn test_l1_anchor_constructor() {
        let anchor = L1Anchor::new(42, 500, 999);
        assert_eq!(anchor.chain_id, 42);
        assert_eq!(anchor.block_height, 500);
        assert_eq!(anchor.timestamp, 999);
    }
}
