//! Solana settlement adapter (STUB).
//!
//! Solana has native Rust programs and cheap calldata.
//! Production would use a Solana program (Anchor) for proof verification.
//!
//! Phase 0: All methods return `NotImplemented`.

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

/// Solana settlement adapter.
///
/// This is a stub implementation. Solana's native Rust programs and
/// high throughput make it an attractive settlement target. Future
/// implementation would use an Anchor program for on-chain proof
/// verification and SPL tokens for bridging.
pub struct SolanaAdapter;

#[async_trait]
impl SettlementLayer for SolanaAdapter {
    fn chain_id(&self) -> &'static str {
        "solana"
    }

    async fn post_batch(&self, _batch_data: &[u8]) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Solana adapter requires Anchor program".into(),
        ))
    }

    async fn verify_proof(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        _proof: &[u8],
    ) -> Result<bool, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Solana proof verification requires native program".into(),
        ))
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        Err(SettlementError::NotImplemented(
            "Solana state root requires program account".into(),
        ))
    }

    async fn deposit(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Solana bridging requires SPL token program".into(),
        ))
    }

    async fn request_withdrawal(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Solana bridging requires SPL token program".into(),
        ))
    }

    async fn submit_batch(&self, _bundle: &ProofBundle) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Solana batch submission requires Anchor program".into(),
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_solana_chain_id() {
        let adapter = SolanaAdapter;
        assert_eq!(adapter.chain_id(), "solana");
    }

    #[tokio::test]
    async fn test_solana_post_batch_not_implemented() {
        let adapter = SolanaAdapter;
        let result = adapter.post_batch(b"test").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Solana")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_solana_verify_proof_not_implemented() {
        let adapter = SolanaAdapter;
        let result = adapter.verify_proof(&[0u8; 32], &[1u8; 32], &[0xAA]).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Solana")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_solana_latest_state_root_not_implemented() {
        let adapter = SolanaAdapter;
        let result = adapter.latest_state_root().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Solana")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_solana_deposit_not_implemented() {
        let adapter = SolanaAdapter;
        let result = adapter.deposit("did:omnia:test", 100).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Solana")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_solana_request_withdrawal_not_implemented() {
        let adapter = SolanaAdapter;
        let result = adapter.request_withdrawal("did:omnia:test", 100).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Solana")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_solana_submit_batch_not_implemented() {
        use crate::proof_bundle::L1Anchor;
        let adapter = SolanaAdapter;
        let bundle = ProofBundle::new(
            [0u8; 32],
            [1u8; 32],
            vec![],
            [0u8; 32],
            L1Anchor::new(1, 0, 0),
        );
        let result = adapter.submit_batch(&bundle).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Solana")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }
}
