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
//! ## Adapters
//!
//! - **Ethereum**: Full implementation (simulated) with Solidity contract
//! - **Bitcoin**: Stub (returns `NotImplemented`)
//! - **Solana**: Stub (returns `NotImplemented`)
//! - **Celestia**: Stub (returns `NotImplemented`)

pub mod circuit;
pub mod operator;
pub mod proof;
pub mod proof_bundle;
pub mod settlement;

// Re-export the core trait and adapters
pub use settlement::{
    BitcoinAdapter, CelestiaAdapter, EthereumAdapter, SettlementError, SettlementLayer,
    SolanaAdapter,
};

// Re-export proof bundle types
pub use proof_bundle::{L1Anchor, ProofBundle, ProofBundleError};
