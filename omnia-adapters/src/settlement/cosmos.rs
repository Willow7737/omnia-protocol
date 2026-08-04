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
/// Roadmap-only stub: every legacy [`SettlementLayer`] operation returns
/// [`SettlementError::NotImplemented`].
/// Do not wire this adapter into production settlement selection.
///
/// This is a stub implementation. Cosmos SDK chains with CosmWasm
/// support can host ZK proof verification contracts. Future implementation
/// would deploy a CosmWasm contract and use IBC for cross-chain
/// asset transfers.
#[deprecated(
    note = "roadmap-only settlement stub; all operations return NotImplemented and it is not production selectable"
)]
pub struct CosmosAdapter;

#[async_trait]
#[allow(deprecated)]
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
