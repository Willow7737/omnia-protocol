//! # Omnia Adapters — ZK-rollup proof system and settlement adapters for L1 bridges
//!
//! This crate implements the ZK-rollup bridge between Omnia's L2 state machine
//! and any L1 settlement layer. The core architecture is **settlement-agnostic**:
//! the L2 state machine (causal graph, consensus, shard router, state root
//! computation) knows nothing about Ethereum, Bitcoin, Solana, or any specific
//! chain. Settlement is handled through pluggable adapters.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │         OMNIA L2 STATE MACHINE          │
//! │  Causal Graph + Consensus + Shards      │
//! │  (L1-agnostic — never changes)          │
//! └─────────────────────────────────────────┘
//!                     ↑
//!          ┌──────────┴──────────┐
//!          ↓                     ↓
//!     ┌─────────┐         ┌─────────┐
//!     │Ethereum │         │ Bitcoin │  (or Solana, Celestia, Cosmos, Noop)
//!     │Adapter  │         │Adapter  │
//!     └─────────┘         └─────────┘
//! ```
//!
//! ## Hybrid Settlement Architecture
//!
//! The settlement layer uses a **hybrid architecture** that preserves MSRV 1.88
//! for the core protocol while enabling full Ethereum functionality when needed:
//!
//! - **`SettlementAdapter` trait**: Core protocol depends ONLY on this trait
//!   (zero alloy, zero MSRV conflict)
//! - **`MockSettlementAdapter`**: Always compiles with Rust 1.88 (MSRV)
//! - **`EthereumSettlementAdapter`**: Feature-gated behind `ethereum-live`,
//!   requires rustc >= 1.91 (alloy dependency)
//! - **`FfiSettlementAdapter`**: Feature-gated behind `settlement-ffi`,
//!   enables production deployments with any Rust version via C library
//!
//! ## ZK Proof System (feature-gated: `arkworks`)
//!
//! The rollup uses Groth16 proofs over the Bn254 curve. All ZK-related code
//! is feature-gated behind the `arkworks` feature to preserve MSRV 1.88
//! compliance when ZK functionality is not needed.

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![warn(missing_docs)]

// ---------------------------------------------------------------------------
// Core modules (always available, MSRV 1.88 compliant)
// ---------------------------------------------------------------------------

pub mod proof_bundle;
pub mod settlement;

// ---------------------------------------------------------------------------
// ZK modules (feature-gated: require arkworks dependencies)
// ---------------------------------------------------------------------------

#[cfg(feature = "arkworks")]
pub mod batch_proof_circuit;
#[cfg(feature = "arkworks")]
pub mod circuit;
pub mod merkle;
#[cfg(feature = "arkworks")]
pub mod operator;
#[cfg(feature = "arkworks")]
pub mod poseidon;
#[cfg(feature = "arkworks")]
pub mod proof;
#[cfg(feature = "arkworks")]
pub mod prover;
#[cfg(feature = "arkworks")]
pub mod setup;

// ---------------------------------------------------------------------------
// Re-exports: Settlement (always available)
// ---------------------------------------------------------------------------

// Core settlement trait and mock (always available)
pub use settlement::{
    FinalityProof, MockSettlementAdapter, SettlementAdapter, SettlementError, SettlementLayer, StateRoot, TxHash,
};

// Legacy settlement adapters (always available)
pub use settlement::{
    BitcoinAdapter, CelestiaAdapter, CosmosAdapter, EthereumAdapter, EthereumConfig, EthereumMode,
    NoopSettlementAdapter, SolanaAdapter,
};

// Conditionally re-export live Ethereum adapter
#[cfg(feature = "ethereum-live")]
pub use settlement::EthereumSettlementAdapter;

// Conditionally re-export FFI adapter
#[cfg(feature = "settlement-ffi")]
pub use settlement::FfiSettlementAdapter;

// ---------------------------------------------------------------------------
// Re-exports: Proof bundle (always available)
// ---------------------------------------------------------------------------

pub use proof_bundle::{L1Anchor, ProofBundle, ProofBundleError};

// ---------------------------------------------------------------------------
// Re-exports: ZK types (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "arkworks")]
pub use batch_proof_circuit::{BatchProofCircuit, BATCH_PROOF_TARGET_SIZE};
#[cfg(feature = "arkworks")]
pub use circuit::{EventWitness, ExpandedRollupCircuit, MerklePathWitness, OperationType};

// MerkleProof and BLAKE3 tree functions are always available
pub use merkle::{build_merkle_tree, compute_root_from_proof, MerkleProof};

// Field-element functions require arkworks
#[cfg(feature = "arkworks")]
pub use merkle::{fr_to_hash, hash_to_fr, poseidon_hash_to_fr};

#[cfg(feature = "arkworks")]
#[allow(deprecated)]
pub use prover::verify_proofs_batch;
#[cfg(feature = "arkworks")]
pub use prover::{verify_multiple, Proof, ProverError, ProvingKey, VerifyingKey};

#[cfg(feature = "arkworks")]
pub use poseidon::ZkError;
