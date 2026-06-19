//! Bitcoin settlement adapter (STUB).
//!
//! Bitcoin has no native smart contract support for proof verification.
//! Production options:
//! - Taproot scripts with limited verification
//! - Lightning Network for fast bridging
//! - Sidechains (Liquid, RSK) for smart contract compatibility
//! - BitVM for optimistic verification
//!
//! Phase 0: All methods return `NotImplemented`.

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

/// Bitcoin settlement adapter.
///
/// This is a stub implementation that returns `NotImplemented` for all
/// operations. Bitcoin's lack of general-purpose smart contracts makes
/// ZK proof verification challenging. Future adapters may leverage
/// BitVM for optimistic verification or sidechains like RSK/Liquid
/// for smart contract support.
pub struct BitcoinAdapter;

#[async_trait]
impl SettlementLayer for BitcoinAdapter {
    fn chain_id(&self) -> &'static str {
        "bitcoin"
    }

    async fn post_batch(&self, _batch_data: &[u8]) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Bitcoin DA requires OP_RETURN or Lightning".into(),
        ))
    }

    async fn verify_proof(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        _proof: &[u8],
    ) -> Result<bool, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Bitcoin proof verification requires BitVM or sidechain".into(),
        ))
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        Err(SettlementError::NotImplemented(
            "Bitcoin has no native state root storage".into(),
        ))
    }

    async fn deposit(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Bitcoin bridging requires Lightning or atomic swaps".into(),
        ))
    }

    async fn request_withdrawal(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Bitcoin bridging requires Lightning or atomic swaps".into(),
        ))
    }

    async fn submit_batch(&self, _bundle: &ProofBundle) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Bitcoin batch submission requires BitVM or sidechain".into(),
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_bitcoin_chain_id() {
        let adapter = BitcoinAdapter;
        assert_eq!(adapter.chain_id(), "bitcoin");
    }

    #[tokio::test]
    async fn test_bitcoin_post_batch_not_implemented() {
        let adapter = BitcoinAdapter;
        let result = adapter.post_batch(b"test").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Bitcoin")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_verify_proof_not_implemented() {
        let adapter = BitcoinAdapter;
        let result = adapter.verify_proof(&[0u8; 32], &[1u8; 32], &[0xAA]).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Bitcoin")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_latest_state_root_not_implemented() {
        let adapter = BitcoinAdapter;
        let result = adapter.latest_state_root().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Bitcoin")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_deposit_not_implemented() {
        let adapter = BitcoinAdapter;
        let result = adapter.deposit("did:omnia:test", 100).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Bitcoin")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_request_withdrawal_not_implemented() {
        let adapter = BitcoinAdapter;
        let result = adapter.request_withdrawal("did:omnia:test", 100).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Bitcoin")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_bitcoin_submit_batch_not_implemented() {
        use crate::proof_bundle::L1Anchor;
        let adapter = BitcoinAdapter;
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
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Bitcoin")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }
}
