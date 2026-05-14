//! # Omnia Protocol — Layer 3: The Binding Layer
//!
//! The Binding Layer replaces trusted oracles with cryptographic self-proof.
//! It bridges physical reality and the digital causal graph by combining
//! three pillars:
//!
//! 1. **RF Fingerprinting** — Physical device identity via unclonable
//!    radio frequency signatures (PUF/RF-DNA).
//! 2. **Quantum-Resistant Commitments** — Long-term integrity using
//!    hybrid classical + post-quantum cryptography (CRYSTALS-Dilithium).
//! 3. **Supply Chain Provenance** — Append-only CRDT provenance logs
//!    that record the complete chain of custody for physical items.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  PhysicalAnchor                       │
//! │  ┌───────────────┐  ┌──────────────┐  ┌───────────┐ │
//! │  │ RfFingerprint  │  │ QuantumCommit│  │ Provenance│ │
//! │  │ (Physical ID)  │  │ (Integrity)  │  │ Log (CRDT)│ │
//! │  └───────────────┘  └──────────────┘  └───────────┘ │
//! └──────────────────────┬──────────────────────────────┘
//!                        │
//!         ┌──────────────┴──────────────┐
//!         │     ProvenanceTracker       │
//!         │  (Integration with shards)  │
//!         └──────────────┬──────────────┘
//!                        │
//!     ┌──────────────────┴──────────────────┐
//!     │  Layer 2: PhysicalShard (existing)  │
//!     └──────────────────┬──────────────────┘
//!                        │
//!     ┌──────────────────┴──────────────────┐
//!     │  Layer 1: Substrate (CausalGraph)   │
//!     └─────────────────────────────────────┘
//! ```
//!
//! # Constraints
//!
//! - **RF fingerprinting** is a stub — real implementation needs hardware
//!   (SDR, spectrum analyzer).
//! - **Quantum commitments** are hybrid (classical + PQC stub) — real PQC
//!   requires `pqc_dilithium` and `pqc_kyber` crates.
//! - **Provenance log** is append-only — no deletes, no edits.
//! - All physical events are anchored in the causal graph — no off-chain
//!   state.
//! - Does not modify `substrate/` or `shards/` core logic.

#![warn(missing_docs)]

pub mod anchor;
pub mod physical_shard;
pub mod provenance;
pub mod quantum_commit;
pub mod rf_fingerprint;

// Re-export core types for convenience
pub use anchor::PhysicalAnchor;
pub use physical_shard::{ProvenanceTracker, ProvenanceTrackerError};
pub use provenance::{ProvenanceEvent, ProvenanceEventType, ProvenanceLog};
pub use quantum_commit::{BindingError, CommitmentPhase, PqPublicKey, QuantumCommitment};
pub use rf_fingerprint::{hamming_distance, RfFingerprint};

/// Semantic version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Layer identifier
pub const LAYER: u8 = 3;
