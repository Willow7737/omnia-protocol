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
/// Roadmap-only stub: every legacy [`SettlementLayer`] operation returns
/// [`SettlementError::NotImplemented`](super::SettlementError::NotImplemented).
/// Do not wire this adapter into production settlement selection.
///
/// This is a stub implementation that returns `NotImplemented` for all
/// operations. Bitcoin's lack of general-purpose smart contracts makes
/// ZK proof verification challenging. Future adapters may leverage
/// BitVM for optimistic verification or sidechains like RSK/Liquid
/// for smart contract support.
#[deprecated(
    note = "roadmap-only settlement stub; all operations return NotImplemented and it is not production selectable"
)]
pub struct BitcoinAdapter;

#[async_trait]
#[allow(deprecated)]
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
