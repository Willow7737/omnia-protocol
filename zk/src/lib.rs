//! # Omnia ZK-Rollup — Phase 0: Settlement-Agnostic
//!
//! This crate implements the ZK-rollup bridge between Omnia's L2 state machine
//! and any L1 settlement layer. The core architecture is **settlement-agnostic**:
//! the L2 state machine (causal graph, consensus, shard router, state root
//! computation) knows nothing about Ethereum, Bitcoin, Solana, or any specific
//! chain. Settlement is handled through pluggable adapters that implement the
//! [`SettlementLayer`] trait.
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
//!     │Ethereum │         │ Bitcoin │  (or Solana, Celestia, etc.)
//!     │Adapter  │         │Adapter  │
//!     └─────────┘         └─────────┘
//! ```
//!
//! ## ZK Proof System
//!
//! The rollup uses Groth16 proofs over the Bn254 curve:
//!
//! - **Circuit**: [`circuit::RollupCircuit`] enforces that
//!   `new_state_root == expected_new_state_root`
//! - **Expanded Circuit**: [`circuit::ExpandedRollupCircuit`]
//!   adds Merkle path verification and per-event state transition constraints
//! - **Prover**: The [`prover`] module handles trusted setup, proof creation,
//!   and verification
//! - **Operator**: [`operator::RollupOperator`] integrates
//!   proof generation into the batch pipeline
//!
//! ## Adapters
//!
//! - **Ethereum**: Full implementation (simulated) with Solidity contract
//! - **Bitcoin**: Stub (returns `NotImplemented`)
//! - **Solana**: Stub (returns `NotImplemented`)
//! - **Celestia**: Stub (returns `NotImplemented`)

pub mod circuit;
pub mod merkle;
pub mod operator;
pub mod poseidon;
pub mod proof;
pub mod proof_bundle;
pub mod prover;
pub mod settlement;
pub mod setup;

// Re-export the core trait and adapters
pub use settlement::{
    BitcoinAdapter, CelestiaAdapter, EthereumAdapter, SettlementError, SettlementLayer,
    SolanaAdapter,
};

// Re-export proof bundle types
pub use proof_bundle::{L1Anchor, ProofBundle, ProofBundleError};

// Re-export prover types for convenience
pub use prover::{Proof, ProverError, ProvingKey, VerifyingKey};

// Re-export expanded circuit types
pub use circuit::{EventWitness, ExpandedRollupCircuit, MerklePathWitness};

// Re-export merkle types
pub use merkle::{
    build_merkle_tree, compute_root_from_proof, fr_to_hash, hash_to_fr, poseidon_hash_to_fr,
    MerkleProof,
};
