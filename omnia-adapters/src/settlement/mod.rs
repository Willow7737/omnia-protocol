//! Settlement layer abstraction — the L1-agnostic core.
//!
//! The [`SettlementLayer`] trait defines the interface that any L1 adapter
//! must implement. The L2 state machine interacts with L1 exclusively through
//! this trait, ensuring that switching settlement layers requires zero changes
//! to consensus, the causal graph, or shard logic.

use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

pub mod bitcoin;
pub mod celestia;
pub mod cosmos;
pub mod ethereum;
pub mod noop;
pub mod solana;

pub use bitcoin::BitcoinAdapter;
pub use celestia::CelestiaAdapter;
pub use cosmos::CosmosAdapter;
pub use ethereum::{EthereumAdapter, EthereumConfig, EthereumMode};
pub use noop::NoopSettlementAdapter;
pub use solana::SolanaAdapter;

/// A pluggable L1 settlement adapter.
///
/// Implementors handle data availability, proof verification, and bridging
/// for a specific L1. The L2 core (causal graph + consensus + shards) is
/// completely unchanged regardless of which adapter is used.
#[async_trait]
pub trait SettlementLayer: Send + Sync {
    /// Human-readable chain identifier.
    fn chain_id(&self) -> &'static str;

    /// Post batch data to L1 for data availability.
    /// Returns an L1-specific reference (tx hash, block height, etc.).
    async fn post_batch(&self, batch_data: &[u8]) -> Result<String, SettlementError>;

    /// Verify a ZK proof on L1.
    /// Returns true if the proof is valid according to L1 consensus rules.
    async fn verify_proof(
        &self,
        old_root: &[u8; 32],
        new_root: &[u8; 32],
        proof: &[u8],
    ) -> Result<bool, SettlementError>;

    /// Get the latest confirmed state root from L1.
    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError>;

    /// Bridge: lock assets on L1, credit on L2.
    async fn deposit(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError>;

    /// Bridge: initiate withdrawal from L2 to L1.
    async fn request_withdrawal(
        &self,
        l2_did: &str,
        amount: u64,
    ) -> Result<String, SettlementError>;

    /// Submit a proof bundle to the L1 for verification and settlement.
    async fn submit_batch(&self, bundle: &ProofBundle) -> Result<String, SettlementError>;
}

/// Errors that can occur during settlement operations.
#[derive(Debug, thiserror::Error)]
pub enum SettlementError {
    /// Error communicating with the L1 RPC endpoint.
    #[error("L1 RPC error: {0}")]
    RpcError(String),
    /// Proof was rejected by the L1 verifier.
    #[error("Proof verification failed: {0}")]
    ProofVerificationFailed(String),
    /// Error during bridge operation (deposit or withdrawal).
    #[error("Bridge error: {0}")]
    BridgeError(String),
    /// The operation is not yet implemented for this chain.
    #[error("Not implemented for chain: {0}")]
    NotImplemented(String),
    /// Configuration error (invalid parameters, missing fields, etc.).
    #[error("Settlement config error: {0}")]
    ConfigError(String),
    /// A submitted transaction failed (reverted or was rejected by the node).
    #[error("Transaction failed: {0}")]
    TxFailed(String),
    /// A submitted transaction timed out before receiving the required confirmations.
    #[error("Transaction timed out after {0} confirmations")]
    TxTimedOut(u64),
    /// Smart contract call returned an error or unexpected data.
    #[error("Contract error: {0}")]
    ContractError(String),
}
