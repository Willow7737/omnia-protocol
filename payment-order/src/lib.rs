//! # Omnia Payment Order State Machine
//!
//! 25-state payment lifecycle per Financial Specification §8.2, §8.3, §15.
//!
//! ## Architecture
//!
//! - [`state`] — 25 `PaymentState` variants + transition matrix
//! - [`types`] — `PaymentOrder` struct (all Spec §8.3 fields)
//! - [`engine`] — `PaymentEngine`: state transitions, authorization, idempotency
//! - [`risk`] — Circuit-breaker limits per Spec §15
//! - [`provider`] — Provider adapter trait, quote types, subsidy tracking (Spec §8.1, §8.4, §8.5)
//! - [`reconciliation`] — Double-entry ledger, 6-way reconciliation, daily controls (Spec §14)
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

pub mod auth;
pub mod engine;
pub mod error;
pub mod ghana_provider;
pub mod governance;
pub mod merchant;
pub mod persistence;
pub mod provider;
pub mod quote_service;
pub mod reconciliation;
pub mod risk;
pub mod state;
pub mod treasury_adapter;
pub mod types;

pub use auth::{Credential, JwtClaims, ServiceRoleRegistry};
pub use engine::{Clock, FixedClock, PaymentEngine, SystemClock};
pub use error::PaymentError;
pub use ghana_provider::GhanaSandboxProvider;
pub use persistence::{InMemoryPaymentStore, PaymentStore, SideEffect, recover_order};
pub use provider::{
    CallbackStatus, CallbackVerification, MobileMoneyProvider, OmniaQuote, ProviderAdapter, ProviderCallback,
    SubsidyRecord, SubsidyTracker,
};
pub use reconciliation::{
    CheckResult, DailyReconciliationReport, Discrepancy, DiscrepancyStatus, LedgerEntry, LedgerEntryType,
    OrderReconciliation, ReconciliationCheck, ReconciliationStatus, ReconciliationSummary, Resolution,
};
pub use risk::{CircuitBreaker, RiskLimits};
pub use state::PaymentState;
pub use quote_service::{QuoteRequest, QuoteService, QuoteServiceConfig, SignedQuote};
pub use treasury_adapter::{TreasuryBridgeAdapter, TreasuryBridgeResult};
pub use types::PaymentOrder;
