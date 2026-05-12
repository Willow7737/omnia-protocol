//! Solana settlement adapter (STUB).
//!
//! Solana has native Rust programs and cheap calldata.
//! Production would use a Solana program (Anchor) for proof verification.
//!
//! Phase 0: All methods return `NotImplemented`.

use super::{SettlementError, SettlementLayer};
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

    async fn request_withdrawal(
        &self,
        _l2_did: &str,
        _amount: u64,
    ) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Solana bridging requires SPL token program".into(),
        ))
    }
}
