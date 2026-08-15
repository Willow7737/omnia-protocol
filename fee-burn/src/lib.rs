//! # OMNIA Fee Calculation and Burn Policy
//!
//! Per Financial Specification §7:
//!
//! - **§7.1 Fee Separation**: UBC and OMNIA fees remain distinct.
//!   8 activity types with defined fee paths.
//! - **§7.2 OMNIA Fee Formula**: `user_fee = base + priority + service`;
//!   `burned = base × burn_ratio`.
//! - **§7.3 Burn Policy**: Initial 0–5% burn ratio; governance ceiling 10–25%.
//!   Every burn emits an event. UBC MUST NOT be burned as OMNIA.
//!
//! ## Modules
//!
//! - [`fee`] — OMNIA fee formula, activity types, fee calculation
//! - [`burn`] — Burn policy, burn ratio governance, burn accounting
//! - [`supply_api`] — Supply query API (minted, burned, circulating)
//! - [`error`] — Error types

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod burn;
pub mod error;
pub mod fee;
pub mod supply_api;

pub use burn::{BurnAccounting, BurnPolicy, BurnRatio};
pub use error::FeeError;
pub use fee::{ActivityType, FeeCalculation, FeeFormula, FeeResult, OmniaFeeSchedule};
pub use supply_api::SupplySnapshot;
