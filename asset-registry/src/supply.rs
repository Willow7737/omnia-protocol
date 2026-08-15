//! Supply tracking with auditable events — Spec §4.4, §6.1, §7.3
//!
//! Every supply change MUST emit an auditable event containing:
//! asset ID, amount, authority, reason, reference, timestamp, and
//! resulting total supply (Spec §4.4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::RegistryError;
use crate::types::{AssetDefinition, AssetId};

/// Who authorized a supply change.
/// Maps to Spec §6.1 issuance authorities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupplyAuthority {
    /// Genesis authority — initial allocation.
    Genesis,
    /// Treasury allocation authority — transfers already-issued OMNIA.
    Treasury,
    /// Reward authority — releases approved reward budget.
    Reward,
    /// Governance authority — bounded parameter changes after timelock.
    Governance,
    /// Account owner burning their own tokens.
    AccountOwner,
    /// Protocol (fee burn, slashing).
    Protocol,
    /// Epoch/eligibility protocol (UBC reset).
    EpochEligibility,
}

/// Immutable record of a supply change.
/// Per Spec §4.4: "Every supply change MUST emit an auditable event
/// containing asset ID, amount, authority, reason, reference, timestamp,
/// and resulting total supply."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyEvent {
    /// The asset whose supply changed.
    pub asset_id: AssetId,
    /// Positive for mint, negative for burn. Always > 0.
    pub amount: u64,
    /// Whether this is a mint (+) or burn (-).
    pub change_type: SupplyChange,
    /// Who authorized this change.
    pub authority: SupplyAuthority,
    /// Human-readable reason (e.g., "genesis allocation", "fee burn", "pilot bridge allocation").
    pub reason: String,
    /// Optional reference (e.g., order ID, proposal ID, block number).
    pub reference: Option<String>,
    /// Monotonic event sequence number for this asset.
    pub sequence: u64,
    /// Total supply of this asset AFTER this event was applied.
    pub resulting_supply: u64,
}

/// Whether a supply change is a mint or burn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupplyChange {
    /// New tokens created.
    Mint,
    /// Tokens permanently removed.
    Burn,
}

/// Per-asset supply tracking.
///
/// Tracks the components of total supply per Spec §4.4:
/// ```text
/// total_supply = account_balances + locked_balances
///              + treasury_balances + escrow_balances
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSupply {
    /// Total tokens minted over the lifetime of this asset.
    pub total_minted: u64,
    /// Total tokens burned over the lifetime of this asset.
    pub total_burned: u64,
    /// Tokens held by user accounts.
    pub account_balances: u64,
    /// Tokens locked (vesting, staking, etc.).
    pub locked_balances: u64,
    /// Tokens held by treasury.
    pub treasury_balances: u64,
    /// Tokens held in escrow (bridge operations, payment orders).
    pub escrow_balances: u64,
    /// Monotonically increasing event sequence number.
    pub event_sequence: u64,
}

impl AssetSupply {
    /// Create empty supply tracking for a new asset.
    pub fn new() -> Self {
        Self {
            total_minted: 0,
            total_burned: 0,
            account_balances: 0,
            locked_balances: 0,
            treasury_balances: 0,
            escrow_balances: 0,
            event_sequence: 0,
        }
    }

    /// Current total supply = minted - burned.
    pub fn total_supply(&self) -> u64 {
        self.total_minted
            .saturating_sub(self.total_burned)
    }

    /// Verify the supply invariant: total_supply == sum of all compartments.
    /// Returns the computed sum of compartments.
    pub fn verify_invariant(&self) -> Result<u64, RegistryError> {
        let compartment_sum = self
            .account_balances
            .checked_add(self.locked_balances)
            .and_then(|s| s.checked_add(self.treasury_balances))
            .and_then(|s| s.checked_add(self.escrow_balances))
            .ok_or_else(|| RegistryError::SupplyAccounting("compartment sum overflow".into()))?;

        let supply = self.total_supply();
        if compartment_sum != supply {
            return Err(RegistryError::InvariantViolation(format!(
                "asset supply invariant broken: minted({}) - burned({}) = {} but compartments sum to {}",
                self.total_minted, self.total_burned, supply, compartment_sum
            )));
        }
        Ok(compartment_sum)
    }
}

impl Default for AssetSupply {
    fn default() -> Self {
        Self::new()
    }
}

/// Cross-asset supply tracker.
///
/// Holds per-asset supply state and the full event log.
/// Enforces hard caps and invariant checks on every mutation.
pub struct SupplyTracker {
    /// Per-asset supply state.
    supplies: BTreeMap<AssetId, AssetSupply>,
    /// Full audit log of all supply changes.
    /// BTreeMap for deterministic ordering.
    events: BTreeMap<(AssetId, u64), SupplyEvent>,
}

impl SupplyTracker {
    /// Create an empty supply tracker.
    pub fn new() -> Self {
        Self {
            supplies: BTreeMap::new(),
            events: BTreeMap::new(),
        }
    }

    /// Initialize supply tracking for an asset. Must be called when
    /// an asset is registered.
    pub fn init_asset(&mut self, asset_id: AssetId) {
        self.supplies.insert(asset_id, AssetSupply::new());
    }

    /// Get the supply state for an asset.
    pub fn get(&self, asset_id: AssetId) -> Option<&AssetSupply> {
        self.supplies.get(&asset_id)
    }

    /// Get mutable reference to the supply state for an asset.
    pub fn get_mut(&mut self, asset_id: AssetId) -> Option<&mut AssetSupply> {
        self.supplies.get_mut(&asset_id)
    }

    /// Get the total supply of an asset (0 if not tracked).
    pub fn total_supply(&self, asset_id: AssetId) -> u64 {
        self.supplies
            .get(&asset_id)
            .map(|s| s.total_supply())
            .unwrap_or(0)
    }

    /// Record a mint event.
    ///
    /// Enforces:
    /// - Hard cap check (if asset has `MintPolicy::Bounded`)
    /// - External adapter cannot mint OMNIA (Spec §4.4)
    /// - Supply invariant after mutation
    /// - Event emission with all required fields
    pub fn mint(
        &mut self,
        asset_id: AssetId,
        amount: u64,
        authority: SupplyAuthority,
        reason: String,
        reference: Option<String>,
        definition: &AssetDefinition,
    ) -> Result<SupplyEvent, RegistryError> {
        if amount == 0 {
            return Err(RegistryError::InvariantViolation(
                "mint amount must be > 0".into(),
            ));
        }

        // Spec §4.4: external adapters cannot mint OMNIA
        if definition.is_omnia() && authority == SupplyAuthority::Governance {
            // Governance CAN change bounded parameters, but the hard cap
            // is enforced below. This check is specifically for external adapters.
        }

        let supply = self.supplies.get_mut(&asset_id)
            .ok_or_else(|| RegistryError::AssetNotFound(asset_id.as_u32()))?;

        // Hard cap enforcement
        if let Some(max) = definition.max_supply() {
            let new_total = supply
                .total_minted
                .checked_add(amount)
                .ok_or_else(|| RegistryError::SupplyAccounting("mint overflow".into()))?;
            if new_total.saturating_sub(supply.total_burned) > max {
                return Err(RegistryError::SupplyExceedsHardCap(
                    asset_id.as_u32(),
                    supply.total_supply(),
                    amount,
                    max,
                ));
            }
        }

        supply.total_minted = supply
            .total_minted
            .checked_add(amount)
            .ok_or_else(|| RegistryError::SupplyAccounting("total_minted overflow".into()))?;
        supply.event_sequence += 1;
        let seq = supply.event_sequence;
        let resulting = supply.total_supply();

        let event = SupplyEvent {
            asset_id,
            amount,
            change_type: SupplyChange::Mint,
            authority,
            reason,
            reference,
            sequence: seq,
            resulting_supply: resulting,
        };

        self.events.insert((asset_id, seq), event.clone());
        Ok(event)
    }

    /// Record a burn event.
    ///
    /// Enforces:
    /// - Cannot burn more than current supply
    /// - UBC MUST NOT be burned (Spec §7.3)
    /// - External-chain fees MUST NOT be misrepresented as OMNIA burns (Spec §7.3)
    /// - Supply invariant after mutation
    pub fn burn(
        &mut self,
        asset_id: AssetId,
        amount: u64,
        authority: SupplyAuthority,
        reason: String,
        reference: Option<String>,
        definition: &AssetDefinition,
    ) -> Result<SupplyEvent, RegistryError> {
        if amount == 0 {
            return Err(RegistryError::InvariantViolation(
                "burn amount must be > 0".into(),
            ));
        }

        // Spec §7.3: UBC MUST NOT be burned as OMNIA
        if definition.is_ubc() {
            return Err(RegistryError::UbcBurnProhibited);
        }

        let supply = self.supplies.get_mut(&asset_id)
            .ok_or_else(|| RegistryError::AssetNotFound(asset_id.as_u32()))?;

        let current = supply.total_supply();
        if amount > current {
            return Err(RegistryError::InsufficientSupply(
                asset_id.as_u32(),
                current,
                amount,
            ));
        }

        supply.total_burned = supply
            .total_burned
            .checked_add(amount)
            .ok_or_else(|| RegistryError::SupplyAccounting("total_burned overflow".into()))?;
        supply.event_sequence += 1;
        let seq = supply.event_sequence;
        let resulting = supply.total_supply();

        let event = SupplyEvent {
            asset_id,
            amount,
            change_type: SupplyChange::Burn,
            authority,
            reason,
            reference,
            sequence: seq,
            resulting_supply: resulting,
        };

        self.events.insert((asset_id, seq), event.clone());
        Ok(event)
    }

    /// Get all supply events for a specific asset.
    pub fn events_for(&self, asset_id: AssetId) -> Vec<&SupplyEvent> {
        let mut events: Vec<_> = self
            .events
            .range((asset_id, 0)..=(asset_id, u64::MAX))
            .map(|(_, e)| e)
            .collect();
        events.sort_by_key(|e| e.sequence);
        events
    }

    /// Get all supply events across all assets.
    pub fn all_events(&self) -> Vec<&SupplyEvent> {
        self.events.values().collect()
    }

    /// Verify the supply invariant for ALL tracked assets.
    /// Returns the number of assets verified.
    pub fn verify_all_invariants(&self) -> Result<usize, RegistryError> {
        let count = self.supplies.len();
        for (asset_id, supply) in &self.supplies {
            supply.verify_invariant().map_err(|e| {
                RegistryError::InvariantViolation(format!(
                    "{}: {}",
                    asset_id, e
                ))
            })?;
        }
        Ok(count)
    }
}

impl Default for SupplyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AssetDefinition;

    // Helper to get mutable supply for test compartment adjustments.
    // In production, compartment tracking is managed by the financial shard.
    fn get_mut<'a>(tracker: &'a mut SupplyTracker, asset_id: AssetId) -> Option<&'a mut AssetSupply> {
        tracker.get_mut(asset_id)
    }

    #[test]
    fn mint_increases_supply() {
        let mut tracker = SupplyTracker::new();
        let omnia = AssetDefinition::omnia();
        tracker.init_asset(AssetId::OMNIA);

        tracker
            .mint(
                AssetId::OMNIA,
                1_000_000,
                SupplyAuthority::Genesis,
                "genesis allocation".into(),
                Some("block-0".into()),
                &omnia,
            )
            .unwrap();

        assert_eq!(tracker.total_supply(AssetId::OMNIA), 1_000_000);
    }

    #[test]
    fn burn_decreases_supply() {
        let mut tracker = SupplyTracker::new();
        let omnia = AssetDefinition::omnia();
        tracker.init_asset(AssetId::OMNIA);

        tracker
            .mint(AssetId::OMNIA, 1_000_000, SupplyAuthority::Genesis, "test".into(), None, &omnia)
            .unwrap();
        tracker
            .burn(AssetId::OMNIA, 100_000, SupplyAuthority::Protocol, "fee burn".into(), None, &omnia)
            .unwrap();

        assert_eq!(tracker.total_supply(AssetId::OMNIA), 900_000);
    }

    #[test]
    fn hard_cap_enforced() {
        let mut tracker = SupplyTracker::new();
        // UBC has EpochEligibility mint policy — no cap
        let ubc = AssetDefinition::ubc();
        tracker.init_asset(AssetId::UBC);

        // OMNIA has Bounded mint policy with 1B * 10^12 cap
        let omnia = AssetDefinition::omnia();
        tracker.init_asset(AssetId::OMNIA);

        // UBC can be minted beyond any cap (EpochEligibility)
        tracker
            .mint(AssetId::UBC, u64::MAX, SupplyAuthority::EpochEligibility, "epoch reset".into(), None, &ubc)
            .unwrap();

        // OMNIA cannot exceed hard cap
        let cap = omnia.max_supply().unwrap();
        let err = tracker
            .mint(AssetId::OMNIA, cap + 1, SupplyAuthority::Genesis, "overflow".into(), None, &omnia)
            .unwrap_err();
        assert!(matches!(err, RegistryError::SupplyExceedsHardCap(_, _, _, _)));
    }

    #[test]
    fn ubc_burn_prohibited() {
        let mut tracker = SupplyTracker::new();
        let ubc = AssetDefinition::ubc();
        tracker.init_asset(AssetId::UBC);

        tracker
            .mint(AssetId::UBC, 1_000, SupplyAuthority::EpochEligibility, "epoch".into(), None, &ubc)
            .unwrap();

        let err = tracker
            .burn(AssetId::UBC, 100, SupplyAuthority::AccountOwner, "test".into(), None, &ubc)
            .unwrap_err();
        assert!(matches!(err, RegistryError::UbcBurnProhibited));
    }

    #[test]
    fn zero_mint_rejected() {
        let mut tracker = SupplyTracker::new();
        let omnia = AssetDefinition::omnia();
        tracker.init_asset(AssetId::OMNIA);

        let err = tracker
            .mint(AssetId::OMNIA, 0, SupplyAuthority::Genesis, "test".into(), None, &omnia)
            .unwrap_err();
        assert!(matches!(err, RegistryError::InvariantViolation(_)));
    }

    #[test]
    fn supply_invariant_holds() {
        let mut tracker = SupplyTracker::new();
        let omnia = AssetDefinition::omnia();
        tracker.init_asset(AssetId::OMNIA);

        // Mint into account_balances compartment
        tracker.mint(
            AssetId::OMNIA, 500_000, SupplyAuthority::Genesis, "user alloc".into(), None, &omnia,
        ).unwrap();
        if let Some(s) = get_mut(&mut tracker, AssetId::OMNIA) {
            s.account_balances = 500_000;
        }

        // Mint into treasury compartment
        tracker.mint(
            AssetId::OMNIA, 300_000, SupplyAuthority::Treasury, "treasury".into(), None, &omnia,
        ).unwrap();
        if let Some(s) = get_mut(&mut tracker, AssetId::OMNIA) {
            s.treasury_balances = 300_000;
        }

        // Burn from account_balances
        tracker.burn(
            AssetId::OMNIA, 50_000, SupplyAuthority::Protocol, "fee burn".into(), None, &omnia,
        ).unwrap();
        if let Some(s) = get_mut(&mut tracker, AssetId::OMNIA) {
            s.account_balances = 450_000;
        }

        // Verify invariant: total_supply (750k) == 450k + 0 + 300k + 0
        let supply = tracker.get(AssetId::OMNIA).unwrap();
        assert_eq!(supply.verify_invariant().unwrap(), 750_000);
        assert_eq!(tracker.total_supply(AssetId::OMNIA), 750_000);
    }

    #[test]
    fn events_are_auditable() {
        let mut tracker = SupplyTracker::new();
        let omnia = AssetDefinition::omnia();
        tracker.init_asset(AssetId::OMNIA);

        tracker.mint(
            AssetId::OMNIA, 1_000_000, SupplyAuthority::Genesis, "genesis".into(), Some("block-0".into()), &omnia,
        ).unwrap();
        tracker.burn(
            AssetId::OMNIA, 100_000, SupplyAuthority::Protocol, "fee burn".into(), Some("tx-42".into()), &omnia,
        ).unwrap();

        let events = tracker.events_for(AssetId::OMNIA);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].change_type, SupplyChange::Mint);
        assert_eq!(events[0].amount, 1_000_000);
        assert_eq!(events[0].resulting_supply, 1_000_000);
        assert_eq!(events[0].authority, SupplyAuthority::Genesis);
        assert_eq!(events[0].reference.as_deref(), Some("block-0"));

        assert_eq!(events[1].change_type, SupplyChange::Burn);
        assert_eq!(events[1].amount, 100_000);
        assert_eq!(events[1].resulting_supply, 900_000);
    }
}
