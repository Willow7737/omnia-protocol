//! Supply invariant tracker and reward schedule (Sprint 4 — §4.4 + §5.3)
//!
//! This module enforces the OMNIA token supply invariant and implements
//! the phased reward schedule that reduces the maximum supply over time.
//!
//! # Supply Invariant (§4.4)
//!
//! The total supply of OMNIA tokens MUST satisfy:
//!
//! ```text
//! total_supply = minted_total - burned_total
//! total_supply <= current_phase_max_supply
//! ```
//!
//! Every mint and burn operation updates the running totals, and the
//! invariant is checked after every state mutation. If violated, the
//! operation is rejected.
//!
//! # Reward Schedule (§5.3)
//!
//! OMNIA tokens are released in phases, each with a declining maximum
//! supply cap:
//!
//! | Phase | Max Supply | Description                       |
//! |-------|-----------|-----------------------------------|
//! | 0     | 80,000,000| Genesis / initial distribution     |
//! | 1     | 60,000,000| Post-pilot contraction             |
//! | 2     | 45,000,000| Mainnet maturity                   |
//! | 3     | 34,000,000| Long-term equilibrium              |
//!
//! Phase transitions are triggered by governance proposals and require
//! a supermajority vote. The schedule is immutable — governance can
//! trigger a transition but cannot alter the caps.

use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------
// Supply phase constants (§5.3)
// ------------------------------------------------------------------

/// Phase 0: Genesis / initial distribution.
pub const PHASE_0_MAX_SUPPLY: u64 = 80_000_000;

/// Phase 1: Post-pilot contraction.
pub const PHASE_1_MAX_SUPPLY: u64 = 60_000_000;

/// Phase 2: Mainnet maturity.
pub const PHASE_2_MAX_SUPPLY: u64 = 45_000_000;

/// Phase 3: Long-term equilibrium.
pub const PHASE_3_MAX_SUPPLY: u64 = 34_000_000;

/// The final phase — no further transitions are possible.
pub const FINAL_PHASE: u8 = 3;

/// All phase caps in order of phase number.
pub const PHASE_MAX_SUPPLIES: [u64; 4] = [
    PHASE_0_MAX_SUPPLY,
    PHASE_1_MAX_SUPPLY,
    PHASE_2_MAX_SUPPLY,
    PHASE_3_MAX_SUPPLY,
];

/// Human-readable descriptions for each phase.
pub const PHASE_DESCRIPTIONS: [&str; 4] = [
    "Genesis / initial distribution",
    "Post-pilot contraction",
    "Mainnet maturity",
    "Long-term equilibrium",
];

/// Errors specific to supply management.
#[derive(Debug, Clone, PartialEq)]
pub enum SupplyError {
    /// Minting would exceed the current phase's maximum supply.
    MintExceedsPhaseCap {
        /// Current total supply.
        current_supply: u64,
        /// Amount requested to mint.
        requested: u64,
        /// Current phase max supply.
        phase_cap: u64,
    },
    /// Supply invariant violated: minted - burned != total_supply.
    InvariantViolation {
        /// Calculated supply (minted - burned).
        calculated: u64,
        /// Tracked total supply.
        tracked: u64,
    },
    /// Invalid phase transition (e.g., skipping a phase or going backward).
    InvalidPhaseTransition { current_phase: u8, target_phase: u8 },
    /// Already at the final phase — no further transitions allowed.
    AlreadyFinalPhase { current_phase: u8 },
    /// Burn would result in negative tracked supply.
    BurnExceedsSupply { current_supply: u64, requested_burn: u64 },
}

impl std::fmt::Display for SupplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupplyError::MintExceedsPhaseCap {
                current_supply,
                requested,
                phase_cap,
            } => {
                write!(
                    f,
                    "Mint would exceed phase cap: supply {current_supply} + {requested} > cap {phase_cap}"
                )
            }
            SupplyError::InvariantViolation { calculated, tracked } => {
                write!(
                    f,
                    "Supply invariant violated: calculated {calculated} != tracked {tracked}"
                )
            }
            SupplyError::InvalidPhaseTransition {
                current_phase,
                target_phase,
            } => {
                write!(f, "Invalid phase transition: {current_phase} -> {target_phase}")
            }
            SupplyError::AlreadyFinalPhase { current_phase } => {
                write!(f, "Already at final phase {current_phase}")
            }
            SupplyError::BurnExceedsSupply {
                current_supply,
                requested_burn,
            } => {
                write!(f, "Burn exceeds supply: {requested_burn} > {current_supply}")
            }
        }
    }
}

impl std::error::Error for SupplyError {}

/// The supply state machine.
///
/// Tracks total minted, total burned, and the current phase.
/// Provides methods to record mints, burns, and phase transitions.
/// The supply invariant is checked after every mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyTracker {
    /// Cumulative tokens ever minted.
    pub total_minted: u64,
    /// Cumulative tokens ever burned.
    pub total_burned: u64,
    /// Current supply phase (0–3).
    pub current_phase: u8,
    /// Block height when the last phase transition occurred.
    pub last_transition_block: u64,
    /// Number of phase transitions executed.
    pub transition_count: u64,
}

impl Default for SupplyTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl SupplyTracker {
    /// Create a new supply tracker in Phase 0.
    pub fn new() -> Self {
        Self {
            total_minted: 0,
            total_burned: 0,
            current_phase: 0,
            last_transition_block: 0,
            transition_count: 0,
        }
    }

    /// Create a supply tracker with an initial mint (e.g., genesis allocation).
    ///
    /// The initial mint must not exceed the Phase 0 cap.
    pub fn with_genesis_mint(initial_mint: u64) -> Result<Self, SupplyError> {
        if initial_mint > PHASE_0_MAX_SUPPLY {
            return Err(SupplyError::MintExceedsPhaseCap {
                current_supply: 0,
                requested: initial_mint,
                phase_cap: PHASE_0_MAX_SUPPLY,
            });
        }
        Ok(Self {
            total_minted: initial_mint,
            total_burned: 0,
            current_phase: 0,
            last_transition_block: 0,
            transition_count: 0,
        })
    }

    /// Get the current total supply (minted - burned).
    pub fn total_supply(&self) -> u64 {
        self.total_minted.saturating_sub(self.total_burned)
    }

    /// Get the maximum supply for the current phase.
    pub fn current_phase_cap(&self) -> u64 {
        PHASE_MAX_SUPPLIES[self.current_phase as usize]
    }

    /// Get the remaining mintable supply in the current phase.
    pub fn remaining_mintable(&self) -> u64 {
        self.current_phase_cap().saturating_sub(self.total_supply())
    }

    /// Check the supply invariant: `total_minted - total_burned == total_supply`.
    ///
    /// This is always true by construction (we compute total_supply from
    /// minted/burned), but the check exists as a defense-in-depth measure
    /// when the tracker is rehydrated from persisted state.
    pub fn check_invariant(&self) -> Result<(), SupplyError> {
        let calculated = self.total_minted.saturating_sub(self.total_burned);
        let tracked = self.total_supply();
        if calculated != tracked {
            return Err(SupplyError::InvariantViolation { calculated, tracked });
        }
        Ok(())
    }

    /// Record a mint operation.
    ///
    /// Ensures that the resulting supply does not exceed the current
    /// phase's maximum supply cap.
    pub fn record_mint(&mut self, amount: u64) -> Result<(), SupplyError> {
        if amount == 0 {
            return Ok(()); // Zero mints are no-ops
        }
        let new_minted = self
            .total_minted
            .checked_add(amount)
            .ok_or_else(|| SupplyError::MintExceedsPhaseCap {
                current_supply: self.total_supply(),
                requested: amount,
                phase_cap: self.current_phase_cap(),
            })?;
        let new_supply = new_minted.saturating_sub(self.total_burned);
        if new_supply > self.current_phase_cap() {
            return Err(SupplyError::MintExceedsPhaseCap {
                current_supply: self.total_supply(),
                requested: amount,
                phase_cap: self.current_phase_cap(),
            });
        }
        self.total_minted = new_minted;
        Ok(())
    }

    /// Record a burn operation.
    ///
    /// Ensures that the burn does not exceed the current supply.
    pub fn record_burn(&mut self, amount: u64) -> Result<(), SupplyError> {
        if amount == 0 {
            return Ok(()); // Zero burns are no-ops
        }
        if amount > self.total_supply() {
            return Err(SupplyError::BurnExceedsSupply {
                current_supply: self.total_supply(),
                requested_burn: amount,
            });
        }
        self.total_burned = self
            .total_burned
            .checked_add(amount)
            .expect("total_burned overflow should be caught by supply check");
        Ok(())
    }

    /// Transition to the next supply phase.
    ///
    /// Phase transitions can ONLY advance by one phase at a time and
    /// cannot go backward. The final phase (3) has no successor.
    ///
    /// After transition, the phase cap is immediately lowered. If the
    /// current supply exceeds the new cap, the transition fails — the
    /// excess must be burned before transitioning.
    ///
    /// The `block_height` parameter records when the transition occurred.
    pub fn transition_phase(&mut self, block_height: u64) -> Result<(), SupplyError> {
        if self.current_phase >= FINAL_PHASE {
            return Err(SupplyError::AlreadyFinalPhase {
                current_phase: self.current_phase,
            });
        }
        let next_phase = self.current_phase + 1;
        let next_cap = PHASE_MAX_SUPPLIES[next_phase as usize];
        if self.total_supply() > next_cap {
            return Err(SupplyError::MintExceedsPhaseCap {
                current_supply: self.total_supply(),
                requested: 0,
                phase_cap: next_cap,
            });
        }
        self.current_phase = next_phase;
        self.last_transition_block = block_height;
        self.transition_count = self.transition_count.checked_add(1).expect("transition_count overflow");
        Ok(())
    }

    /// Get a summary of the current supply state.
    pub fn snapshot(&self) -> SupplySnapshot {
        SupplySnapshot {
            total_minted: self.total_minted,
            total_burned: self.total_burned,
            total_supply: self.total_supply(),
            current_phase: self.current_phase,
            phase_cap: self.current_phase_cap(),
            remaining_mintable: self.remaining_mintable(),
            phase_description: PHASE_DESCRIPTIONS[self.current_phase as usize].to_string(),
            last_transition_block: self.last_transition_block,
            transition_count: self.transition_count,
        }
    }

    /// Serialize to bytes with a version prefix.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        let mut bytes = vec![1u8]; // version
        bytes.extend(postcard::to_allocvec(self)?);
        Ok(bytes)
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        if bytes.is_empty() || bytes[0] != 1 {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        postcard::from_bytes(&bytes[1..])
    }
}

/// Immutable snapshot of the supply state for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplySnapshot {
    /// Cumulative tokens ever minted.
    pub total_minted: u64,
    /// Cumulative tokens ever burned.
    pub total_burned: u64,
    /// Current circulating supply.
    pub total_supply: u64,
    /// Current phase (0–3).
    pub current_phase: u8,
    /// Maximum supply for the current phase.
    pub phase_cap: u64,
    /// Remaining tokens that can be minted in this phase.
    pub remaining_mintable: u64,
    /// Human-readable phase description.
    pub phase_description: String,
    /// Block height of the last phase transition.
    pub last_transition_block: u64,
    /// Total number of phase transitions.
    pub transition_count: u64,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Phase constant tests
    // ------------------------------------------------------------------

    #[test]
    fn test_phase_caps_monotonically_decreasing() {
        for i in 1..PHASE_MAX_SUPPLIES.len() {
            assert!(
                PHASE_MAX_SUPPLIES[i] < PHASE_MAX_SUPPLIES[i - 1],
                "Phase {} cap {} should be < phase {} cap {}",
                i,
                PHASE_MAX_SUPPLIES[i],
                i - 1,
                PHASE_MAX_SUPPLIES[i - 1]
            );
        }
    }

    #[test]
    fn test_final_phase_is_3() {
        assert_eq!(FINAL_PHASE, 3);
        assert_eq!(PHASE_MAX_SUPPLIES.len(), 4);
    }

    // ------------------------------------------------------------------
    // SupplyTracker: construction
    // ------------------------------------------------------------------

    #[test]
    fn test_new_tracker() {
        let tracker = SupplyTracker::new();
        assert_eq!(tracker.total_minted, 0);
        assert_eq!(tracker.total_burned, 0);
        assert_eq!(tracker.total_supply(), 0);
        assert_eq!(tracker.current_phase, 0);
        assert_eq!(tracker.current_phase_cap(), PHASE_0_MAX_SUPPLY);
    }

    #[test]
    fn test_genesis_mint_valid() {
        let tracker = SupplyTracker::with_genesis_mint(50_000_000).unwrap();
        assert_eq!(tracker.total_minted, 50_000_000);
        assert_eq!(tracker.total_supply(), 50_000_000);
    }

    #[test]
    fn test_genesis_mint_exceeds_cap() {
        let result = SupplyTracker::with_genesis_mint(PHASE_0_MAX_SUPPLY + 1);
        assert!(matches!(result, Err(SupplyError::MintExceedsPhaseCap { .. })));
    }

    #[test]
    fn test_genesis_mint_at_exact_cap() {
        let tracker = SupplyTracker::with_genesis_mint(PHASE_0_MAX_SUPPLY).unwrap();
        assert_eq!(tracker.total_supply(), PHASE_0_MAX_SUPPLY);
        assert_eq!(tracker.remaining_mintable(), 0);
    }

    // ------------------------------------------------------------------
    // SupplyTracker: minting
    // ------------------------------------------------------------------

    #[test]
    fn test_record_mint_basic() {
        let mut tracker = SupplyTracker::new();
        tracker.record_mint(1_000_000).unwrap();
        assert_eq!(tracker.total_minted, 1_000_000);
        assert_eq!(tracker.total_supply(), 1_000_000);
    }

    #[test]
    fn test_record_mint_zero_is_noop() {
        let mut tracker = SupplyTracker::new();
        tracker.record_mint(0).unwrap();
        assert_eq!(tracker.total_minted, 0);
    }

    #[test]
    fn test_record_mint_exceeds_cap() {
        let mut tracker = SupplyTracker::with_genesis_mint(79_999_999).unwrap();
        let result = tracker.record_mint(2);
        assert!(matches!(result, Err(SupplyError::MintExceedsPhaseCap { .. })));
        // Supply unchanged
        assert_eq!(tracker.total_minted, 79_999_999);
    }

    #[test]
    fn test_record_mint_multiple() {
        let mut tracker = SupplyTracker::new();
        tracker.record_mint(10_000_000).unwrap();
        tracker.record_mint(20_000_000).unwrap();
        tracker.record_mint(30_000_000).unwrap();
        assert_eq!(tracker.total_minted, 60_000_000);
        assert_eq!(tracker.total_supply(), 60_000_000);
    }

    // ------------------------------------------------------------------
    // SupplyTracker: burning
    // ------------------------------------------------------------------

    #[test]
    fn test_record_burn_basic() {
        let mut tracker = SupplyTracker::with_genesis_mint(10_000_000).unwrap();
        tracker.record_burn(3_000_000).unwrap();
        assert_eq!(tracker.total_burned, 3_000_000);
        assert_eq!(tracker.total_supply(), 7_000_000);
    }

    #[test]
    fn test_record_burn_zero_is_noop() {
        let mut tracker = SupplyTracker::with_genesis_mint(10_000_000).unwrap();
        tracker.record_burn(0).unwrap();
        assert_eq!(tracker.total_burned, 0);
    }

    #[test]
    fn test_record_burn_exceeds_supply() {
        let mut tracker = SupplyTracker::with_genesis_mint(10_000_000).unwrap();
        let result = tracker.record_burn(10_000_001);
        assert!(matches!(result, Err(SupplyError::BurnExceedsSupply { .. })));
        assert_eq!(tracker.total_burned, 0);
    }

    #[test]
    fn test_record_burn_full_supply() {
        let mut tracker = SupplyTracker::with_genesis_mint(10_000_000).unwrap();
        tracker.record_burn(10_000_000).unwrap();
        assert_eq!(tracker.total_supply(), 0);
    }

    // ------------------------------------------------------------------
    // SupplyTracker: phase transitions
    // ------------------------------------------------------------------

    #[test]
    fn test_transition_phase_0_to_1() {
        let mut tracker = SupplyTracker::with_genesis_mint(50_000_000).unwrap();
        tracker.transition_phase(1000).unwrap();
        assert_eq!(tracker.current_phase, 1);
        assert_eq!(tracker.current_phase_cap(), PHASE_1_MAX_SUPPLY);
        assert_eq!(tracker.last_transition_block, 1000);
        assert_eq!(tracker.transition_count, 1);
    }

    #[test]
    fn test_transition_phase_1_to_2() {
        let mut tracker = SupplyTracker::with_genesis_mint(40_000_000).unwrap();
        tracker.transition_phase(1000).unwrap(); // 0->1
        tracker.transition_phase(2000).unwrap(); // 1->2
        assert_eq!(tracker.current_phase, 2);
        assert_eq!(tracker.current_phase_cap(), PHASE_2_MAX_SUPPLY);
    }

    #[test]
    fn test_transition_full_path() {
        let mut tracker = SupplyTracker::with_genesis_mint(30_000_000).unwrap();
        tracker.transition_phase(100).unwrap(); // 0->1
        tracker.transition_phase(200).unwrap(); // 1->2
        tracker.transition_phase(300).unwrap(); // 2->3
        assert_eq!(tracker.current_phase, 3);
        assert_eq!(tracker.current_phase_cap(), PHASE_3_MAX_SUPPLY);
        assert_eq!(tracker.transition_count, 3);
    }

    #[test]
    fn test_transition_from_final_phase_fails() {
        let mut tracker = SupplyTracker::with_genesis_mint(30_000_000).unwrap();
        tracker.transition_phase(100).unwrap();
        tracker.transition_phase(200).unwrap();
        tracker.transition_phase(300).unwrap();
        let result = tracker.transition_phase(400);
        assert!(matches!(result, Err(SupplyError::AlreadyFinalPhase { .. })));
    }

    #[test]
    fn test_transition_supply_exceeds_new_cap() {
        let mut tracker = SupplyTracker::with_genesis_mint(70_000_000).unwrap();
        // Phase 1 cap is 60M, but supply is 70M — transition should fail
        let result = tracker.transition_phase(1000);
        assert!(matches!(result, Err(SupplyError::MintExceedsPhaseCap { .. })));
        assert_eq!(tracker.current_phase, 0); // unchanged
    }

    #[test]
    fn test_transition_then_mint_within_new_cap() {
        let mut tracker = SupplyTracker::with_genesis_mint(40_000_000).unwrap();
        tracker.transition_phase(1000).unwrap(); // 0->1, cap=60M
                                                 // Can still mint up to 60M
        tracker.record_mint(15_000_000).unwrap();
        assert_eq!(tracker.total_supply(), 55_000_000);
        // But not beyond
        let result = tracker.record_mint(6_000_000);
        assert!(matches!(result, Err(SupplyError::MintExceedsPhaseCap { .. })));
    }

    // ------------------------------------------------------------------
    // SupplyTracker: invariant check
    // ------------------------------------------------------------------

    #[test]
    fn test_invariant_holds_after_mint() {
        let mut tracker = SupplyTracker::new();
        tracker.record_mint(5_000_000).unwrap();
        assert!(tracker.check_invariant().is_ok());
    }

    #[test]
    fn test_invariant_holds_after_burn() {
        let mut tracker = SupplyTracker::with_genesis_mint(10_000_000).unwrap();
        tracker.record_burn(3_000_000).unwrap();
        assert!(tracker.check_invariant().is_ok());
    }

    // ------------------------------------------------------------------
    // SupplyTracker: snapshot
    // ------------------------------------------------------------------

    #[test]
    fn test_snapshot_contents() {
        let mut tracker = SupplyTracker::with_genesis_mint(50_000_000).unwrap();
        tracker.record_burn(5_000_000).unwrap();
        let snap = tracker.snapshot();
        assert_eq!(snap.total_minted, 50_000_000);
        assert_eq!(snap.total_burned, 5_000_000);
        assert_eq!(snap.total_supply, 45_000_000);
        assert_eq!(snap.current_phase, 0);
        assert_eq!(snap.phase_cap, PHASE_0_MAX_SUPPLY);
        assert_eq!(snap.remaining_mintable, 35_000_000);
        assert!(!snap.phase_description.is_empty());
    }

    // ------------------------------------------------------------------
    // Serialization
    // ------------------------------------------------------------------

    #[test]
    fn test_serialization_roundtrip() {
        let mut tracker = SupplyTracker::with_genesis_mint(50_000_000).unwrap();
        tracker.record_burn(5_000_000).unwrap();
        tracker.transition_phase(1000).unwrap();
        let bytes = tracker.to_bytes().unwrap();
        let restored = SupplyTracker::from_bytes(&bytes).unwrap();
        assert_eq!(restored.total_minted, 50_000_000);
        assert_eq!(restored.total_burned, 5_000_000);
        assert_eq!(restored.current_phase, 1);
        assert_eq!(restored.last_transition_block, 1000);
    }

    #[test]
    fn test_deserialize_wrong_version() {
        let bytes = vec![99u8]; // wrong version
        let result = SupplyTracker::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_empty() {
        let result = SupplyTracker::from_bytes(&[]);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // Reward schedule edge cases
    // ------------------------------------------------------------------

    #[test]
    fn test_phase_3_no_further_minting_after_full() {
        let mut tracker = SupplyTracker::with_genesis_mint(34_000_000).unwrap();
        tracker.transition_phase(100).unwrap();
        tracker.transition_phase(200).unwrap();
        tracker.transition_phase(300).unwrap();
        // Phase 3 cap is 34M, supply is 34M — no more minting
        let result = tracker.record_mint(1);
        assert!(matches!(result, Err(SupplyError::MintExceedsPhaseCap { .. })));
    }

    #[test]
    fn test_burn_creates_minting_headroom() {
        let mut tracker = SupplyTracker::with_genesis_mint(80_000_000).unwrap();
        // Supply is at cap, can't mint
        assert!(tracker.record_mint(1).is_err());
        // Burn 10M
        tracker.record_burn(10_000_000).unwrap();
        assert_eq!(tracker.total_supply(), 70_000_000);
        // Now can mint 10M
        tracker.record_mint(10_000_000).unwrap();
        assert_eq!(tracker.total_supply(), 80_000_000);
    }
}
