//! Bitcoin settlement adapter.
//!
//! Bitcoin has no native smart contract support for proof verification, so
//! (unlike Ethereum) there is no on-chain path for `submit_batch_with_proof`
//! — the live adapter settles by anchoring state roots as OP_RETURN data
//! instead. See [`live`] for the real implementation (behind the
//! `bitcoin-live` feature) and [`BitcoinAdapter`] below for the legacy,
//! backward-compatible `SettlementLayer` stub.
//!
//! Production options considered for full proof verification, none wired
//! up yet:
//! - Taproot scripts with limited verification
//! - Lightning Network for fast bridging
//! - Sidechains (Liquid, RSK) for smart contract compatibility
//! - BitVM for optimistic verification

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

#[cfg(feature = "bitcoin-live")]
pub mod live;

#[cfg(feature = "bitcoin-live")]
pub use live::{BitcoinConfig, BitcoinSettlementAdapter};

/// Legacy Bitcoin settlement adapter (roadmap-only stub).
///
/// Every [`SettlementLayer`] operation returns
/// [`SettlementError::NotImplemented`]. Kept for backward compatibility;
/// new code should use [`BitcoinSettlementAdapter`] (behind the
/// `bitcoin-live` feature) instead, which implements the modern
/// `SettlementAdapter` trait against a real Bitcoin Core node.
///
/// Unchanged from the original stub — this file was only reorganized
/// from a flat `bitcoin.rs` into this module so `live.rs` could sit
/// alongside it, the same way `ethereum.rs` was restructured earlier.
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
