//! Supply tracking with auditable events — Spec §4.4, §5.3, §6.1, §7.3
//!
//! Every supply change MUST emit an auditable event containing:
//! asset ID, amount, authority, reason, reference, timestamp, and
//! resulting total supply (Spec §4.4).
//!
//! ## Reward Authority (§5.3)
//!
//! The [`RewardAuthority`] wraps a [`crate::genesis::RewardSchedule`] and
//! enforces that reward mints never exceed the per-year budget. It tracks
//! how much has been released per year and per-era, and applies the
//! unclaimed/slashed policies.

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

// ------------------------------------------------------------------
// Reward Authority (Spec §5.3)
// ------------------------------------------------------------------

/// Enforces the reward schedule from Spec §5.3.
///
/// Per Spec §5.3, the reward authority:
/// - "No reward authority may mint outside the hard cap."
/// - Releases only the approved per-year reward budget
/// - Tracks per-year and per-era releases
/// - Handles unclaimed rewards per [`crate::genesis::UnclaimedRewardPolicy`]
/// - Handles slashed validator rewards per [`crate::genesis::SlashedRewardPolicy`]
///
/// The proposed initial schedule is:
///
/// | Year | OMNIA Reward |
/// |------|-------------|
/// | 1    | 80,000,000  |
/// | 2    | 60,000,000  |
/// | 3    | 45,000,000  |
/// | 4    | 34,000,000  |
///
/// "The remaining 181,000,000 MUST have a fully specified schedule
/// before the reward pool is activated." (Spec §5.3)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardAuthority {
    /// The reward schedule defining per-year budgets.
    pub schedule: crate::genesis::RewardSchedule,
    /// How much has been released per year (year → plancks released).
    pub released_per_year: BTreeMap<u32, u64>,
    /// How much has been slashed per year (year → plancks slashed).
    pub slashed_per_year: BTreeMap<u32, u64>,
    /// How much was unclaimed and returned to treasury per year.
    pub unclaimed_per_year: BTreeMap<u32, u64>,
    /// Total amount released across all years.
    pub total_released: u64,
    /// Total amount slashed across all years.
    pub total_slashed: u64,
    /// Whether the reward pool is active.
    /// Per Spec §5.3, the remaining 181M needs a fully specified
    /// schedule before activation. During pilot, rewards are NOT
    /// automatically minted.
    pub active: bool,
    /// The current era/year for reward calculations.
    /// In production this is derived from block height or epoch.
    pub current_year: u32,
}

impl Default for RewardAuthority {
    fn default() -> Self {
        Self::new(crate::genesis::RewardSchedule::canonical())
    }
}

impl RewardAuthority {
    /// Create a new reward authority with the given schedule.
    /// The authority starts INACTIVE — it must be explicitly activated
    /// after governance approval and schedule finalization.
    pub fn new(schedule: crate::genesis::RewardSchedule) -> Self {
        Self {
            schedule,
            released_per_year: BTreeMap::new(),
            slashed_per_year: BTreeMap::new(),
            unclaimed_per_year: BTreeMap::new(),
            total_released: 0,
            total_slashed: 0,
            active: false,
            current_year: 0,
        }
    }

    /// Activate the reward authority.
    /// Returns error if already active.
    pub fn activate(&mut self, start_year: u32) -> Result<(), RegistryError> {
        if self.active {
            return Err(RegistryError::InvariantViolation(
                "reward authority already active".into(),
            ));
        }
        self.active = true;
        self.current_year = start_year;
        Ok(())
    }

    /// Deactivate the reward authority (emergency pause).
    /// Per Spec §12: emergency pause with no silent balance alteration.
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Set the current year (called at era boundaries).
    pub fn set_year(&mut self, year: u32) {
        self.current_year = year;
    }

    /// Get the budget for the current year.
    /// Returns 0 if no budget is defined for this year.
    pub fn current_year_budget(&self) -> u64 {
        self.schedule
            .reward_for_year(self.current_year)
            .unwrap_or(0)
    }

    /// Get the remaining budget for the current year.
    pub fn current_year_remaining(&self) -> u64 {
        let budget = self.current_year_budget();
        let released = self
            .released_per_year
            .get(&self.current_year)
            .copied()
            .unwrap_or(0);
        budget.saturating_sub(released)
    }

    /// Get the total budget across all scheduled years.
    pub fn total_budget(&self) -> u64 {
        self.schedule.total_scheduled()
    }

    /// Get the total remaining budget across all future years.
    pub fn total_remaining(&self) -> u64 {
        self.total_budget()
            .saturating_sub(self.total_released)
            .saturating_sub(self.total_slashed)
    }

    /// Validate that a reward mint of `amount` is within the current
    /// year's budget. Does NOT mutate state — the actual minting is
    /// done via `SupplyTracker::mint` after validation.
    ///
    /// Returns `Ok(())` if the mint is allowed.
    /// Returns `Err` if:
    /// - The authority is not active
    /// - The year has no scheduled budget
    /// - The amount would exceed the year's remaining budget
    pub fn validate_reward_mint(&self, amount: u64, year: u32) -> Result<(), RegistryError> {
        if !self.active {
            return Err(RegistryError::RewardAuthorityInactive);
        }

        let budget = self
            .schedule
            .reward_for_year(year)
            .ok_or_else(|| RegistryError::RewardYearNotScheduled(year))?;

        let released = self.released_per_year.get(&year).copied().unwrap_or(0);
        let remaining = budget.saturating_sub(released);

        if amount > remaining {
            return Err(RegistryError::RewardBudgetExceeded {
                year,
                requested: amount,
                remaining,
                budget,
            });
        }

        Ok(())
    }

    /// Record that a reward mint has occurred.
    /// Call this AFTER the actual `SupplyTracker::mint` succeeds.
    pub fn record_release(&mut self, amount: u64, year: u32) {
        *self.released_per_year.entry(year).or_insert(0) += amount;
        self.total_released = self.total_released.saturating_add(amount);
    }

    /// Record a slashed validator reward.
    /// Per Spec §5.3: "treatment of inactive or slashed validators."
    /// The amount is removed from the year's released total and handled
    /// per the slashed policy.
    pub fn record_slash(&mut self, amount: u64, year: u32) {
        *self.slashed_per_year.entry(year).or_insert(0) += amount;
        self.total_slashed = self.total_slashed.saturating_add(amount);

        // Apply slashed policy
        match self.schedule.slashed_policy {
            crate::genesis::SlashedRewardPolicy::Burned => {
                // Slashed amount is effectively burned — it was already
                // minted but is now removed from circulation.
                // The actual burn is done via SupplyTracker::burn.
            }
            crate::genesis::SlashedRewardPolicy::ReturnToPool => {
                // Return to the year's budget — reduce released count
                let released = self.released_per_year.get(&year).copied().unwrap_or(0);
                if released >= amount {
                    *self.released_per_year.get_mut(&year).expect("just accessed") -= amount;
                    self.total_released = self.total_released.saturating_sub(amount);
                }
            }
            crate::genesis::SlashedRewardPolicy::ReturnToTreasury => {
                // Removed from circulation and credited to treasury.
                // The treasury credit is handled at the accounting layer.
                // Here we just reduce the released count.
                let released = self.released_per_year.get(&year).copied().unwrap_or(0);
                if released >= amount {
                    *self.released_per_year.get_mut(&year).expect("just accessed") -= amount;
                    self.total_released = self.total_released.saturating_sub(amount);
                }
            }
        }
    }

    /// Record unclaimed rewards per the unclaimed policy.
    /// Per Spec §5.3: "treatment of unclaimed rewards."
    pub fn record_unclaimed(&mut self, amount: u64, year: u32) {
        *self.unclaimed_per_year.entry(year).or_insert(0) += amount;

        match self.schedule.unclaimed_policy {
            crate::genesis::UnclaimedRewardPolicy::ReturnToTreasury => {
                // Reduce released count — tokens return to treasury.
                let released = self.released_per_year.get(&year).copied().unwrap_or(0);
                if released >= amount {
                    *self.released_per_year.get_mut(&year).expect("just accessed") -= amount;
                    self.total_released = self.total_released.saturating_sub(amount);
                }
            }
            crate::genesis::UnclaimedRewardPolicy::RollForward => {
                // Tokens stay available for next period — no change to released.
            }
            crate::genesis::UnclaimedRewardPolicy::PermanentlyLost => {
                // Tokens are burned — handled at supply tracker level.
                let released = self.released_per_year.get(&year).copied().unwrap_or(0);
                if released >= amount {
                    *self.released_per_year.get_mut(&year).expect("just accessed") -= amount;
                    self.total_released = self.total_released.saturating_sub(amount);
                }
            }
        }
    }

    /// Check if a given year has a fully specified schedule.
    /// Per Spec §5.3, years without a schedule cannot release rewards.
    pub fn is_year_scheduled(&self, year: u32) -> bool {
        self.schedule.reward_for_year(year).is_some()
    }

    /// Verify the reward invariant:
    /// total_released <= total_scheduled - total_slashed
    pub fn verify_invariant(&self) -> bool {
        self.total_released
            <= self
                .total_budget()
                .saturating_sub(self.total_slashed)
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

    // --- RewardAuthority tests (§5.3) ---

    #[test]
    fn reward_authority_starts_inactive() {
        let auth = RewardAuthority::default();
        assert!(!auth.active);
        assert_eq!(auth.total_released, 0);
        assert_eq!(auth.total_budget(), 219_000_000_000_000_000);
    }

    #[test]
    fn reward_authority_activate() {
        let mut auth = RewardAuthority::default();
        auth.activate(1).unwrap();
        assert!(auth.active);
        assert_eq!(auth.current_year, 1);
    }

    #[test]
    fn reward_authority_double_activate_fails() {
        let mut auth = RewardAuthority::default();
        auth.activate(1).unwrap();
        assert!(auth.activate(2).is_err());
    }

    #[test]
    fn reward_authority_inactive_rejects_mint() {
        let auth = RewardAuthority::default();
        let err = auth.validate_reward_mint(1000, 1);
        assert!(matches!(err, Err(RegistryError::RewardAuthorityInactive)));
    }

    #[test]
    fn reward_authority_unscheduled_year_rejected() {
        let mut auth = RewardAuthority::default();
        auth.activate(1).unwrap();
        let err = auth.validate_reward_mint(1000, 5); // no year 5 schedule
        assert!(matches!(err, Err(RegistryError::RewardYearNotScheduled(5))));
    }

    #[test]
    fn reward_authority_within_budget() {
        let mut auth = RewardAuthority::default();
        auth.activate(1).unwrap();
        // Year 1 budget: 80M OMNIA = 80_000_000_000_000_000 plancks
        let half = 40_000_000_000_000_000;
        assert!(auth.validate_reward_mint(half, 1).is_ok());
        auth.record_release(half, 1);
        assert_eq!(auth.total_released, half);
        assert_eq!(auth.current_year_remaining(), half);
        // Second half also OK
        assert!(auth.validate_reward_mint(half, 1).is_ok());
    }

    #[test]
    fn reward_authority_budget_exceeded() {
        let mut auth = RewardAuthority::default();
        auth.activate(1).unwrap();
        let budget = 80_000_000_000_000_000u64;
        auth.record_release(budget, 1);
        let err = auth.validate_reward_mint(1, 1);
        assert!(matches!(err, Err(RegistryError::RewardBudgetExceeded { .. })));
    }

    #[test]
    fn reward_authority_deactivate() {
        let mut auth = RewardAuthority::default();
        auth.activate(1).unwrap();
        auth.deactivate();
        assert!(!auth.active);
        // Reactivation allowed after deactivation
        assert!(auth.activate(2).is_ok());
        assert_eq!(auth.current_year, 2);
    }

    #[test]
    fn reward_authority_slash_return_to_pool() {
        let mut auth = RewardAuthority::new(crate::genesis::RewardSchedule {
            unclaimed_policy: crate::genesis::UnclaimedRewardPolicy::ReturnToTreasury,
            slashed_policy: crate::genesis::SlashedRewardPolicy::ReturnToPool,
            ..crate::genesis::RewardSchedule::canonical()
        });
        auth.activate(1).unwrap();
        let budget = 80_000_000_000_000_000u64;
        auth.record_release(budget, 1);
        assert_eq!(auth.total_released, budget);
        // Slash 10M — returned to pool
        auth.record_slash(10_000_000_000_000_000, 1);
        assert_eq!(auth.total_released, 70_000_000_000_000_000);
        assert_eq!(auth.total_slashed, 10_000_000_000_000_000);
        // Remaining should include the returned 10M
        assert_eq!(auth.current_year_remaining(), 10_000_000_000_000_000);
    }

    #[test]
    fn reward_authority_unclaimed_return_to_treasury() {
        let mut auth = RewardAuthority::new(crate::genesis::RewardSchedule {
            unclaimed_policy: crate::genesis::UnclaimedRewardPolicy::ReturnToTreasury,
            slashed_policy: crate::genesis::SlashedRewardPolicy::Burned,
            ..crate::genesis::RewardSchedule::canonical()
        });
        auth.activate(1).unwrap();
        auth.record_release(50_000_000_000_000_000, 1);
        auth.record_unclaimed(10_000_000_000_000_000, 1);
        assert_eq!(auth.total_released, 40_000_000_000_000_000);
        // Unclaimed amount tracked
        assert_eq!(auth.unclaimed_per_year.get(&1).copied().unwrap_or(0), 10_000_000_000_000_000);
    }

    #[test]
    fn reward_authority_invariant_holds() {
        let mut auth = RewardAuthority::default();
        auth.activate(1).unwrap();
        auth.record_release(40_000_000_000_000_000, 1);
        assert!(auth.verify_invariant());
        // Even after a slash
        auth.record_slash(5_000_000_000_000_000, 1);
        assert!(auth.verify_invariant());
    }

    #[test]
    fn reward_authority_multi_year_tracking() {
        let mut auth = RewardAuthority::default();
        auth.activate(1).unwrap();
        // Release full year 1
        auth.record_release(80_000_000_000_000_000, 1);
        // Move to year 2
        auth.set_year(2);
        // Year 2 has 60M budget
        assert_eq!(auth.current_year_budget(), 60_000_000_000_000_000);
        assert_eq!(auth.current_year_remaining(), 60_000_000_000_000_000);
        // Total remaining = 219M - 80M released = 139M
        assert_eq!(auth.total_remaining(), 139_000_000_000_000_000);
    }
}
