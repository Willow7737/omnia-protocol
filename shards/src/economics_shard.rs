//! Economics shard — UBC token, useful work, and governance operations
//!
//! Re-exports [`EconomicsOp`] and [`EconomicsState`] from the `omnia-economics`
//! crate, eliminating the former duplicate definition that used `Vec<u8>` for
//! proofs and a separate `EconomicsVoteChoice` enum. The canonical types now
//! live in `omnia_economics`; this module provides backward-compatible aliases.

// Re-export the canonical EconomicsOp from omnia-economics
pub use omnia_economics::EconomicsOp;

/// Backward-compatible alias for `VoteChoice`.
///
/// The shards crate previously defined `EconomicsVoteChoice` with the same
/// variants (`For`, `Against`, `Abstain`). The canonical type is now
/// [`omnia_economics::VoteChoice`]; this alias preserves the old name for
/// any downstream code that references `EconomicsVoteChoice`.
pub type EconomicsVoteChoice = omnia_economics::VoteChoice;
