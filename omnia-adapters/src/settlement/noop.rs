//! Noop settlement adapter for standalone mode.
//!
//! In standalone mode (no L1 settlement), all operations succeed
//! immediately with no-op responses. This is useful for development,
//! testing, and single-node deployments that don't need L1 anchoring.

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

/// No-op settlement adapter for standalone mode.
///
/// All operations succeed immediately without performing any real
/// settlement. Useful for development, testing, and single-node
/// deployments where L1 anchoring is not required.
pub struct NoopSettlementAdapter;

#[async_trait]
impl SettlementLayer for NoopSettlementAdapter {
    fn chain_id(&self) -> &'static str {
        "noop"
    }

    async fn post_batch(&self, _batch_data: &[u8]) -> Result<String, SettlementError> {
        Ok("noop-tx-standalone".to_string())
    }

    async fn verify_proof(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        proof: &[u8],
    ) -> Result<bool, SettlementError> {
        // Accept any non-empty proof in standalone mode
        Ok(!proof.is_empty())
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        Ok([0u8; 32])
    }

    async fn deposit(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        Ok(format!("noop-deposit-{l2_did}-{amount}"))
    }

    async fn request_withdrawal(
        &self,
        l2_did: &str,
        amount: u64,
    ) -> Result<String, SettlementError> {
        Ok(format!("noop-withdrawal-{l2_did}-{amount}"))
    }

    async fn submit_batch(&self, _bundle: &ProofBundle) -> Result<String, SettlementError> {
        Ok("noop-submit-batch-standalone".to_string())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_chain_id() {
        let adapter = NoopSettlementAdapter;
        assert_eq!(adapter.chain_id(), "noop");
    }

    #[tokio::test]
    async fn test_noop_post_batch() {
        let adapter = NoopSettlementAdapter;
        let result = adapter.post_batch(b"test").await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("noop"));
    }

    #[tokio::test]
    async fn test_noop_verify_proof() {
        let adapter = NoopSettlementAdapter;
        // Non-empty proof → true
        assert!(adapter
            .verify_proof(&[0u8; 32], &[1u8; 32], &[0xAA])
            .await
            .unwrap());
        // Empty proof → false
        assert!(!adapter
            .verify_proof(&[0u8; 32], &[1u8; 32], &[])
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_noop_latest_state_root() {
        let adapter = NoopSettlementAdapter;
        let root = adapter.latest_state_root().await.unwrap();
        assert_eq!(root, [0u8; 32]);
    }

    #[tokio::test]
    async fn test_noop_deposit() {
        let adapter = NoopSettlementAdapter;
        let result = adapter.deposit("did:test", 100).await.unwrap();
        assert!(result.contains("did:test"));
        assert!(result.contains("100"));
    }

    #[tokio::test]
    async fn test_noop_withdrawal() {
        let adapter = NoopSettlementAdapter;
        let result = adapter.request_withdrawal("did:test", 50).await.unwrap();
        assert!(result.contains("did:test"));
        assert!(result.contains("50"));
    }
}
