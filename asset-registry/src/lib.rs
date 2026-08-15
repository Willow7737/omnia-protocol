//! # Omnia Asset Registry
//!
//! On-chain asset registry, supply tracking, treasury allocation, and
//! asset-scoped balances per the
//! [Financial Specification](https://github.com/Willow7737/omnia-protocol/blob/main/docs/financial/financial-specification.md).
//!
//! ## Design Principles
//!
//! 1. **Asset-scoped balances**: `balance[asset_id][account_id] = amount`
//! 2. **No cross-asset contamination**: no operation can move one asset as another
//! 3. **Auditable supply changes**: every mint/burn emits a `SupplyChange` event
//! 4. **Hard cap enforcement**: `circulating + locked + treasury + escrow + unissued <= cap`
//! 5. **UBC/OMNIA separation**: UBC cannot become OMNIA; external adapters cannot mint OMNIA
//! 6. **Treasury allocates, never mints**: per Spec §6.1, treasury moves already-issued OMNIA
//!
//! ## Modules
//!
//! - [`types`] — Core asset types (AssetId, AssetDefinition, AssetClass, etc.)
//! - [`registry`] — Asset registration, queries, freeze/unfreeze
//! - [`supply`] — Supply tracking with auditable events
//! - [`treasury`] — Treasury allocation with hard limits (Spec §5.2, §6)
//! - [`balances`] — Asset-scoped balance ledger (Spec §4.3)
//! - [`error`] — Error types
//!
//! ## Invariants (Spec §4.4)
//!
//! ```text
//! For every asset:
//!     total_supply = account_balances
//!                  + locked_balances
//!                  + treasury_balances
//!                  + escrow_balances
//!
//! minted - burned = total_supply_delta
//!
//! no operation can move one asset as another asset
//! no UBC operation can create a transferable OMNIA balance
//! no external-chain adapter can invoke native OMNIA minting
//! no payment callback can create a balance without verified order state
//! no order can allocate more OMNIA than its reserved inventory
//! no failed or refunded order can remain economically delivered
//! ```

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod balances;
pub mod error;
pub mod genesis;
pub mod registry;
pub mod supply;
pub mod treasury;
pub mod types;

pub use error::RegistryError;
pub use genesis::{
    GenesisAllocation, GenesisPlan, IssuanceAuthority, RewardSchedule, TreasuryAccounting,
    TreasuryCategory, UnclaimedRewardPolicy,
};
pub use registry::AssetRegistry;
pub use supply::{SupplyChange, SupplyEvent, SupplyTracker};
