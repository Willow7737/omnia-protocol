//! Cosmos settlement adapter (STUB).
//!
//! Cosmos SDK chains support CosmWasm smart contracts for on-chain
//! verification. Production would deploy a CosmWasm contract for
//! proof verification and use IBC for cross-chain bridging.
//!
//! Phase 0: All methods return `NotImplemented`.

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

/// Cosmos settlement adapter.
///
/// This is a stub implementation. Cosmos SDK chains with CosmWasm
/// support can host ZK proof verification contracts. Future implementation
/// would deploy a CosmWasm contract and use IBC for cross-chain
/// asset transfers.
pub struct CosmosAdapter;

#[async_trait]
impl SettlementLayer for CosmosAdapter {
    fn chain_id(&self) -> &'static str {
        "cosmos"
    }

    async fn post_batch(&self, _batch_data: &[u8]) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Cosmos adapter requires CosmWasm contract deployment".into(),
        ))
    }

    async fn verify_proof(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        _proof: &[u8],
    ) -> Result<bool, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Cosmos proof verification requires CosmWasm contract".into(),
        ))
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        Err(SettlementError::NotImplemented(
            "Cosmos state root requires module query".into(),
        ))
    }

    async fn deposit(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Cosmos bridging requires IBC transfer".into(),
        ))
    }

    async fn request_withdrawal(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Cosmos bridging requires IBC transfer".into(),
        ))
    }

    async fn submit_batch(&self, _bundle: &ProofBundle) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Cosmos batch submission requires CosmWasm contract".into(),
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_cosmos_chain_id() {
        let adapter = CosmosAdapter;
        assert_eq!(adapter.chain_id(), "cosmos");
    }

    #[tokio::test]
    async fn test_cosmos_post_batch_not_implemented() {
        let adapter = CosmosAdapter;
        let result = adapter.post_batch(b"test").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Cosmos")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_cosmos_verify_proof_not_implemented() {
        let adapter = CosmosAdapter;
        let result = adapter.verify_proof(&[0u8; 32], &[1u8; 32], &[0xAA]).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Cosmos")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_cosmos_latest_state_root_not_implemented() {
        let adapter = CosmosAdapter;
        let result = adapter.latest_state_root().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Cosmos")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_cosmos_deposit_not_implemented() {
        let adapter = CosmosAdapter;
        let result = adapter.deposit("did:omnia:test", 100).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Cosmos")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_cosmos_request_withdrawal_not_implemented() {
        let adapter = CosmosAdapter;
        let result = adapter.request_withdrawal("did:omnia:test", 100).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Cosmos")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_cosmos_submit_batch_not_implemented() {
        use crate::proof_bundle::L1Anchor;
        let adapter = CosmosAdapter;
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
            SettlementError::NotImplemented(msg) => assert!(msg.contains("Cosmos")),
            other => panic!("Expected NotImplemented, got {other:?}"),
        }
    }
}
