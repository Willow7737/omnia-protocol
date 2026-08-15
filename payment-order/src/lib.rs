//! # Omnia Payment Order State Machine
//!
//! 24-state payment lifecycle per Financial Specification §8.2, §8.3, §15.
//!
//! ## Architecture
//!
//! - [`state`] — 24 `PaymentState` variants + transition matrix
//! - [`types`] — `PaymentOrder` struct (all Spec §8.3 fields)
//! - [`engine`] — `PaymentEngine`: state transitions, authorization, idempotency
//! - [`risk`] — Circuit-breaker limits per Spec §15
//! - [`error`] — `PaymentError` types
//!
//! ## Key Invariants
//!
//! 1. Terminal states (`DELIVERED`, `REFUNDED`, `CANCELLED`) are absorbing —
//!    no further transitions allowed.
//! 2. Only transitions in the valid transition matrix are allowed.
//! 3. Every transition emits an immutable `StateTransitionEvent`.
//! 4. The client MUST NOT declare payment success (Spec §8.3).
//! 5. Duplicate callbacks, out-of-order events, and reversals are handled
//!    idempotently.
//! 6. No failed or refunded order can remain economically delivered
//!    (Spec §4.4).
//! 7. No order can allocate more OMNIA than its reserved inventory
//!    (Spec §4.4).

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod engine;
pub mod error;
pub mod risk;
pub mod state;
pub mod types;

pub use engine::PaymentEngine;
pub use error::PaymentError;
pub use risk::{CircuitBreaker, RiskLimits};
pub use state::PaymentState;
pub use types::PaymentOrder;
