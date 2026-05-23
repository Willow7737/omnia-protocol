//! Financial shard module
//!
//! The Financial shard handles all value-transfer operations: minting,
//! burning, and transferring assets. It is the most critical shard for
//! Phase 0 because it underpins the UBC token system.
//!
//! Unlike CRDT-based shards, the Financial shard uses **strict causal
//! ordering** for balance updates. Transfers and burns are not commutative
//! — they require the event's vector clock to determine a deterministic
//! processing order.

pub mod ops;
pub mod state;
pub mod validator;

pub use ops::FinancialOp;
pub use state::{AccountBalance, FinancialState};
pub use validator::FinancialValidator;
