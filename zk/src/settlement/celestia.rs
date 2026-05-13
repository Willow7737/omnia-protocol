//! Celestia settlement adapter (STUB).
//!
//! Celestia is data availability only — no proof verification.
//! Production would use Celestia for cheap DA + another L1 for verification.
//!
//! Phase 0: All methods return `NotImplemented`.

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

/// Celestia settlement adapter.
///
/// This is a stub implementation. Celestia provides only data availability —
/// it cannot verify ZK proofs or hold assets. A real deployment would
/// combine Celestia for DA with another L1 (e.g., Ethereum) for proof
/// verification and bridging.
pub struct CelestiaAdapter;

#[async_trait]
impl SettlementLayer for CelestiaAdapter {
    fn chain_id(&self) -> &'static str {
        "celestia"
    }

    async fn post_batch(&self, _batch_data: &[u8]) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia DA requires blob submission via node RPC".into(),
        ))
    }

    async fn verify_proof(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        _proof: &[u8],
    ) -> Result<bool, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia has no proof verification — use with another L1".into(),
        ))
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia has no state root storage".into(),
        ))
    }

    async fn deposit(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia has no asset layer".into(),
        ))
    }

    async fn request_withdrawal(
        &self,
        _l2_did: &str,
        _amount: u64,
    ) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia has no asset layer".into(),
        ))
    }

    async fn submit_batch(&self, _bundle: &ProofBundle) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia batch submission requires blob submission via node RPC".into(),
        ))
    }
}
