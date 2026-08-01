//! Settlement layer abstraction — the L1-agnostic core.
//!
//! This module provides two trait hierarchies for settlement:
//!
//! ## 1. `SettlementAdapter` (new, hybrid architecture)
//!
//! The primary trait for the hybrid settlement architecture. Core protocol
//! depends ONLY on this trait — zero alloy, zero MSRV conflict. Implementations:
//!
//! - **Mock** ([`MockSettlementAdapter`]): Always compiles with Rust 1.88.
//!   Used in all CI pipelines and development. Deterministic BLAKE3-based
//!   responses with simulated latency.
//! - **Live Ethereum** (`EthereumSettlementAdapter`): Feature-gated behind
//!   `ethereum-live`. Requires rustc >= 1.91 (alloy dependency). Connects
//!   to real Ethereum RPC endpoints.
//! - **FFI** (`FfiSettlementAdapter`): Feature-gated behind `settlement-ffi`.
//!   Calls a pre-compiled C library, enabling production deployments with
//!   any Rust version.
//!
//! ## 2. `SettlementLayer` (legacy, retained for backward compatibility)
//!
//! The original trait with full adapter methods (post_batch, verify_proof,
//! deposit, withdrawal, etc.). Existing code using this trait continues
//! to work unchanged.

use crate::merkle::MerkleProof;
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

pub mod bitcoin;
pub mod celestia;
pub mod cosmos;
pub mod ethereum;
pub mod ffi;
pub mod mock;
pub mod noop;
pub mod solana;

// Legacy module (backward-compatible, kept as-is)
// The old ethereum.rs is now at settlement/ethereum/mod.rs which re-exports
// the legacy EthereumAdapter from the parent module.

pub use bitcoin::BitcoinAdapter;
pub use celestia::CelestiaAdapter;
pub use cosmos::CosmosAdapter;
pub use noop::NoopSettlementAdapter;
pub use solana::SolanaAdapter;

// Re-export new hybrid architecture types
pub use mock::MockSettlementAdapter;

// Conditionally re-export FFI adapter (requires both the feature flag
// and the pre-compiled C library to be present at build time).
#[cfg(all(feature = "settlement-ffi", has_settlement_lib))]
pub use ffi::FfiSettlementAdapter;

// Conditionally re-export live Ethereum adapter
#[cfg(feature = "ethereum-live")]
pub use ethereum::EthereumSettlementAdapter;

// ---------------------------------------------------------------------------
// SettlementAdapter trait (hybrid architecture core)
// ---------------------------------------------------------------------------

/// A Groth16 proof in EVM calldata layout (AUDIT-2026-07 C3, #341):
/// 32-byte big-endian words matching `OmniaRollup.submitBatch`'s
/// `uint256[2] a`, `uint256[2][2] b`, `uint256[2] c` parameters. The G2
/// component layout is `[[x.c0, x.c1], [y.c0, y.c1]]` (c0 = real part),
/// as documented on the contract.
///
/// Feature-neutral by design (plain bytes, no arkworks types) so the
/// settlement trait can carry it regardless of which prover features are
/// enabled. Produced by `prover::proof_to_evm` under the `arkworks`
/// feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmProof {
    /// G1 point A: [x, y].
    pub a: [[u8; 32]; 2],
    /// G2 point B: [[x.c0, x.c1], [y.c0, y.c1]].
    pub b: [[[u8; 32]; 2]; 2],
    /// G1 point C: [x, y].
    pub c: [[u8; 32]; 2],
}

/// A pluggable settlement adapter for the hybrid architecture.
///
/// This is the core trait that the Omnia Protocol depends on for
/// settlement. It is intentionally minimal — only three methods
/// plus an optional health check — to keep the core protocol
/// free of any specific L1 dependency (zero alloy, zero MSRV conflict).
///
/// Implementations choose their backend at compile time or runtime:
///
/// | Implementation | Feature Flag | MSRV | Use Case |
/// |---|---|---|---|
/// | `MockSettlementAdapter` | (always) | 1.88 | CI, testing, development |
/// | `EthereumSettlementAdapter` | `ethereum-live` | 1.91+ | Live Ethereum settlement |
/// | `FfiSettlementAdapter` | `settlement-ffi` | 1.88 | Production via C library |
///
/// ## Architecture
///
/// ```text
/// ┌─────────────────────────────────────┐
/// │     OMNIA CORE PROTOCOL             │
/// │  (depends only on SettlementAdapter) │
/// │  (zero alloy, zero MSRV conflict)    │
/// └─────────────────────────────────────┘
///                     │
///          ┌──────────┼──────────┐
///          ▼          ▼          ▼
///     ┌─────────┐ ┌─────────┐ ┌─────────┐
///     │  Mock   │ │ Ethereum│ │   FFI   │
///     │ (1.88)  │ │ (1.91+) │ │ (any)   │
///     └─────────┘ └─────────┘ └─────────┘
/// ```
#[async_trait]
pub trait SettlementAdapter: Send + Sync {
    /// Submit a state root to the settlement layer.
    ///
    /// Returns a transaction hash identifying the submission.
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError>;

    /// Submit a batch with its Groth16 proof and public inputs
    /// (AUDIT-2026-07 C3, #341) — the real settlement path for contracts
    /// that verify proofs on-chain (`OmniaRollup.submitBatch`).
    ///
    /// `public_inputs` are 32-byte big-endian words; for the
    /// `ExpandedRollupCircuit` they are
    /// `[old_state_root, new_state_root, event_commitment]`.
    ///
    /// The default implementation fails closed for adapters whose
    /// settlement layer has no proof-verifying entry point.
    async fn submit_batch_with_proof(
        &self,
        _new_root: [u8; 32],
        _proof: &EvmProof,
        _public_inputs: &[[u8; 32]],
        _batch_data: &[u8],
    ) -> Result<TxHash, SettlementError> {
        Err(SettlementError::ContractError(
            "this settlement adapter does not support proof-carrying batch submission".to_string(),
        ))
    }

    /// Fetch a finality proof for a previously submitted transaction.
    ///
    /// The proof confirms that the transaction has been finalized
    /// on the settlement layer with sufficient confirmations.
    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError>;

    /// Verify a Merkle inclusion proof against the settlement layer.
    ///
    /// Returns `true` if the proof is valid according to the
    /// settlement layer's state.
    ///
    /// # Arguments
    ///
    /// * `leaf` — The 32-byte leaf value being proven
    /// * `proof` — The Merkle inclusion proof
    async fn verify_inclusion(&self, leaf: &[u8; 32], proof: &MerkleProof) -> Result<bool, SettlementError>;

    /// Check whether this adapter is connected to a live settlement layer.
    ///
    /// Returns `false` for mock/stub adapters, `true` for adapters
    /// that interact with real L1 networks.
    fn is_live(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A transaction hash identifying a settlement submission.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TxHash(pub [u8; 32]);

impl std::fmt::Display for TxHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

/// A finality proof confirming a settlement transaction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FinalityProof {
    /// The transaction hash that was finalized.
    pub tx_hash: TxHash,
    /// The block number at which the transaction was included.
    pub block_number: u64,
    /// Number of confirmation blocks.
    pub confirmation_count: u64,
    /// BLAKE3 proof hash binding the finality proof to the transaction.
    pub proof_hash: [u8; 32],
}

/// A Merkle inclusion proof for state verification.
///
/// This type is re-exported from the `merkle` module for convenience.
/// When the `arkworks` feature is disabled, the field-element methods
/// are not available, but the proof structure itself is still usable.
pub type StateRoot = [u8; 32];

// ---------------------------------------------------------------------------
// SettlementLayer trait (legacy, backward-compatible)
// ---------------------------------------------------------------------------

/// A pluggable L1 settlement adapter (legacy trait).
///
/// Implementors handle data availability, proof verification, and bridging
/// for a specific L1. The L2 core (causal graph + consensus + shards) is
/// completely unchanged regardless of which adapter is used.
///
/// **Note**: For new code, prefer using [`SettlementAdapter`] which
/// provides a cleaner, MSRV-safe interface.
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
    async fn request_withdrawal(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError>;

    /// Submit a proof bundle to the L1 for verification and settlement.
    async fn submit_batch(&self, bundle: &ProofBundle) -> Result<String, SettlementError>;
}

// Re-export legacy Ethereum types from the ethereum module
pub use ethereum::{EthereumAdapter, EthereumConfig, EthereumMode};

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
