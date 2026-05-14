//! # Omnia Protocol — Layer 5: Economics
//!
//! The Economics layer implements Universal Basic Compute (UBC), a
//! soulbound token system that guarantees every identity a free monthly
//! quota of transactions and compute. Excess capacity can be contributed
//! to useful work for additional rewards.
//!
//! # Three Pillars
//!
//! 1. **UBC Token** — Non-transferable monthly allowance tied to DID
//! 2. **Proof-of-Useful-Work** — Earn extra compute by contributing
//! 3. **Quadratic Voting + Decay** — Governance that prevents plutocracy
//!
//! # Integration
//!
//! - **Layer 1**: UBC distribution events go into the `CausalGraph`
//! - **Layer 2**: `FinancialShard` tracks UBC balances; `IdentityShard`
//!   maps DIDs to quotas
//! - **Layer 4**: `AgentIdentity` with `GovernanceVote` capability

#![warn(missing_docs)]

pub mod economics_shard;
pub mod error;
pub mod fixed_point;
pub mod governance;
pub mod quota;
pub mod ubc;
pub mod useful_work;

// Re-export core types for convenience
pub use economics_shard::{EconomicsOp, EconomicsState};
pub use error::EconomicsError;
pub use fixed_point::{BasisPpmExt, DecayRate, BASIS_PPM};
pub use governance::{GovernanceState, Proposal, VoteChoice};
pub use quota::{QuotaSystem, DEFAULT_EPOCH_DURATION_MS, DEFAULT_UBC_QUOTA};
pub use ubc::UbcToken;
pub use useful_work::{UsefulWorkProof, UsefulWorkType};
