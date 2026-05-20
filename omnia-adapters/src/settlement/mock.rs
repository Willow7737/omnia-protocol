//! Mock settlement adapter for testing and development.
//!
//! Provides a deterministic, in-process implementation of [`SettlementAdapter`]
//! that requires no external dependencies (no alloy, no RPC endpoint). This
//! adapter always compiles with Rust 1.88 (MSRV) and is used as the default
//! in all CI pipelines.
//!
//! ## Usage
//!
//! The mock adapter simulates realistic network latency (10 ms per call) and
//! returns deterministic BLAKE3-derived transaction hashes. It tracks a
//! monotonically increasing state root internally.
//!
//! ```rust
//! use omnia_adapters::settlement::{SettlementAdapter, MockSettlementAdapter};
//!
//! # async fn example() -> Result<(), omnia_adapters::settlement::SettlementError> {
//! let adapter = MockSettlementAdapter::new();
//! assert!(!adapter.is_live());
//!
//! let tx_hash = adapter.submit_root([1u8; 32]).await?;
//! assert!(!tx_hash.0.iter().all(|&b| b == 0));
//! # Ok(())
//! # }
//! ```

use super::{FinalityProof, SettlementAdapter, SettlementError, TxHash};
use crate::merkle::MerkleProof;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Mock settlement adapter for testing and development.
///
/// All operations succeed deterministically with simulated latency.
/// This adapter is always available (zero alloy dependency) and
/// compiles with Rust 1.88 (MSRV).
pub struct MockSettlementAdapter {
    /// Simulated network latency per call.
    latency: Duration,
    /// Monotonically increasing counter for generating unique tx hashes.
    counter: AtomicU64,
}

impl MockSettlementAdapter {
    /// Create a new mock adapter with default latency (10 ms).
    pub fn new() -> Self {
        Self {
            latency: Duration::from_millis(10),
            counter: AtomicU64::new(0),
        }
    }

    /// Create a new mock adapter with custom latency.
    ///
    /// Useful for testing timeout behavior or simulating slow networks.
    pub fn with_latency(latency: Duration) -> Self {
        Self {
            latency,
            counter: AtomicU64::new(0),
        }
    }

    /// Generate a deterministic mock transaction hash from state root and counter.
    fn mock_tx_hash(&self, root: [u8; 32]) -> TxHash {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut input = root.to_vec();
        input.extend_from_slice(&count.to_le_bytes());
        let hash = blake3::derive_key("OMNIA-MOCK-SETTLEMENT-TX", &input);
        TxHash(hash)
    }
}

impl Default for MockSettlementAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SettlementAdapter for MockSettlementAdapter {
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError> {
        // Simulate network latency
        tokio::time::sleep(self.latency).await;
        let tx_hash = self.mock_tx_hash(root);
        tracing::debug!(
            "[MockSettlement] submit_root: tx_hash=0x{}..",
            hex::encode(&tx_hash.0[..8])
        );
        Ok(tx_hash)
    }

    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError> {
        tokio::time::sleep(self.latency).await;
        // Mock finality proof: use BLAKE3 to derive a deterministic proof
        let proof_hash = blake3::derive_key("OMNIA-MOCK-FINALITY", &tx.0);
        Ok(FinalityProof {
            tx_hash: tx,
            block_number: self.counter.load(Ordering::Relaxed),
            confirmation_count: 3,
            proof_hash,
        })
    }

    async fn verify_inclusion(&self, _proof: &MerkleProof) -> Result<bool, SettlementError> {
        tokio::time::sleep(self.latency).await;
        // Mock: all inclusion proofs are valid
        Ok(true)
    }

    fn is_live(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_submit_root() {
        let adapter = MockSettlementAdapter::with_latency(Duration::from_millis(0));
        let tx_hash = adapter.submit_root([1u8; 32]).await.unwrap();
        // Tx hash should not be all zeros
        assert!(!tx_hash.0.iter().all(|&b| b == 0));
    }

    #[tokio::test]
    async fn test_mock_submit_root_deterministic() {
        let adapter = MockSettlementAdapter::with_latency(Duration::from_millis(0));
        let hash1 = adapter.submit_root([42u8; 32]).await.unwrap();
        let hash2 = adapter.submit_root([42u8; 32]).await.unwrap();
        // Different counter values produce different hashes
        assert_ne!(hash1, hash2);
    }

    #[tokio::test]
    async fn test_mock_fetch_finality() {
        let adapter = MockSettlementAdapter::with_latency(Duration::from_millis(0));
        let tx = TxHash([1u8; 32]);
        let proof = adapter.fetch_finality(tx.clone()).await.unwrap();
        assert_eq!(proof.tx_hash, tx);
        assert_eq!(proof.confirmation_count, 3);
    }

    #[tokio::test]
    async fn test_mock_verify_inclusion() {
        let adapter = MockSettlementAdapter::with_latency(Duration::from_millis(0));
        let proof = MerkleProof {
            siblings: vec![[0u8; 32]],
            directions: vec![true],
        };
        assert!(adapter.verify_inclusion(&proof).await.unwrap());
    }

    #[test]
    fn test_mock_is_not_live() {
        let adapter = MockSettlementAdapter::new();
        assert!(!adapter.is_live());
    }

    #[test]
    fn test_mock_default() {
        let adapter = MockSettlementAdapter::default();
        assert!(!adapter.is_live());
    }
}
