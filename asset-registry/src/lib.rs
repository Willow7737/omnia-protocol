//! # Omnia Asset Registry
//!
//! On-chain asset registry and supply tracking per the
//! [Financial Specification §4](https://github.com/Willow7737/omnia-protocol/blob/main/docs/financial/financial-specification.md).
//!
//! ## Design Principles
//!
//! 1. **Asset-scoped balances**: `balance[asset_id][account_id] = amount`
//! 2. **No cross-asset contamination**: no operation can move one asset as another
//! 3. **Auditable supply changes**: every mint/burn emits a `SupplyChange` event
//! 4. **Hard cap enforcement**: `circulating + locked + treasury + escrow + unissued ≤ cap`
//! 5. **UBC/OMNIA separation**: UBC cannot become OMNIA; external adapters cannot mint OMNIA
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

mod error;
mod registry;
mod supply;
mod types;

pub use error::RegistryError;
pub use registry::AssetRegistry;
pub use supply::{SupplyChange, SupplyEvent, SupplyTracker};
pub use types::*;
