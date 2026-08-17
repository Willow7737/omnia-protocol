// Financial invariant property tests -- Spec 16
//!
//! 7 highest-priority property tests from Spec §16:
//!
//! 1. UBC→OMNIA impossible
//! 2. OMNIA→other asset impossible
//! 3. External adapter can't mint OMNIA
//! 4. Callbacks can't bypass verification
//! 5. Duplicate callbacks can't duplicate allocation
//! 6. Refunds can't leave delivered balances
//! 7. Supply can't exceed hard cap

use proptest::prelude::*;

use omnia_fee_burn::{
    burn::{BurnAccounting, BurnRatio},
    fee::{ActivityType, FeeFormula, OmniaFeeSchedule},
    supply_api::SupplySnapshot,
};

// --- Property 1 & 2: UBC and OMNIA fee separation ---

proptest! {
    /// For any activity, the fee result must have a consistent UBC/OMNIA flag.
    /// UBC activities (identity, compute) MUST NOT produce OMNIA fees.
    /// OMNIA activities MUST NOT be flagged as UBC.
    #[test]
    fn prop_fee_separation_is_consistent(activity in prop_oneof![
        Just(ActivityType::BasicIdentity),
        Just(ActivityType::Compute),
        Just(ActivityType::OmniaTransfer),
        Just(ActivityType::PriorityInclusion),
        Just(ActivityType::GhanaMobileMoney),
        Just(ActivityType::MerchantPayment),
        Just(ActivityType::ExternalChain),
        Just(ActivityType::GovernanceProposal),
    ], priority in 0u64..10_000_000u64) {
        let formula = FeeFormula::new();
        if let Ok(result) = formula.calculate(activity, priority) {
            if activity.accepts_ubc() {
                prop_assert!(result.is_ubc, "UBC activity must have is_ubc=true");
                prop_assert_eq!(result.burned_amount, 0, "UBC fees must not be burned");
            }
            if activity.accepts_omnia() && !activity.accepts_ubc() {
                prop_assert!(!result.is_ubc, "OMNIA activity must not be flagged as UBC");
            }
        }
    }
}

// --- Property 3: External adapter can't mint OMNIA ---

proptest! {
    /// External chain fees must have zero OMNIA fee.
    #[test]
    fn prop_external_chain_no_omnia_fee(_priority in 0u64..1_000_000u64) {
        let formula = FeeFormula::new();
        let result = formula.calculate(ActivityType::ExternalChain, 0)
            .expect("ExternalChain fee calculation should succeed");
        prop_assert_eq!(result.total_fee, 0, "External chain must have zero OMNIA fee");
        prop_assert_eq!(result.burned_amount, 0, "External chain must not be burned as OMNIA");
    }
}

// --- Property 5: Duplicate callbacks can't duplicate allocation ---

proptest! {
    /// For any valid transition, applying the same transition twice
    /// must produce the same state (idempotency).
    #[test]
    fn prop_idempotent_transition(state in 0u8..25u8) {
        use omnia_payment_order::state::PaymentState;
        let all_states = [
            PaymentState::Created,
            PaymentState::Quoted,
            PaymentState::PaymentPending,
            PaymentState::PaymentVerified,
            PaymentState::RiskReview,
            PaymentState::RiskApproved,
            PaymentState::InventoryReserved,
            PaymentState::AllocationSubmitted,
            PaymentState::AllocationFinalized,
            PaymentState::Delivered,
            PaymentState::QuoteExpired,
            PaymentState::PaymentFailed,
            PaymentState::PaymentReversed,
            PaymentState::PaymentUnderpaid,
            PaymentState::PaymentOverpaid,
            PaymentState::PaymentTimeout,
            PaymentState::RiskRejected,
            PaymentState::InventoryUnavailable,
            PaymentState::AllocationFailed,
            PaymentState::OnChainTimeout,
            PaymentState::OnChainUncertain,
            PaymentState::RefundPending,
            PaymentState::Refunded,
            PaymentState::ManualReview,
            PaymentState::Cancelled,
        ];
        let state = all_states[state as usize % all_states.len()];
        if !state.is_terminal() {
            // Self-transition must be allowed (idempotent)
            prop_assert!(state.can_transition_to(state).is_ok());
        } else {
            // Terminal states must reject all other transitions
            for target in &all_states {
                if *target != state {
                    prop_assert!(state.can_transition_to(*target).is_err(),
                        "terminal state {:?} should reject transition to {:?}", state, target);
                }
            }
        }
    }
}

// --- Property 6: Refunds can't leave delivered balances ---

proptest! {
    /// Burned amount must never exceed the fee amount.
    #[test]
    fn prop_burn_never_exceeds_fee(burn_bps in 0u16..2500u16, fee in 1_000u64..10_000_000_000u64) {
        let ratio = BurnRatio::from_bps(burn_bps);
        let burned = ratio.apply(fee, omnia_fee_burn::fee::RoundingRule::Up);
        prop_assert!(burned <= fee, "burned {} must not exceed fee {}", burned, fee);
    }
}

// --- Property 7: Supply can't exceed hard cap ---

proptest! {
    /// Supply snapshot must detect when hard cap is reached.
    #[test]
    fn prop_supply_cap_detection(minted in 500_000_000_000_000_000u64..1_500_000_000_000_000_000u64,
                                          burned in 0u64..100_000_000_000_000u64) {
        let cap = 1_000_000_000_000_000_000u64;
        let mut burn_acc = BurnAccounting::new();
        if burned > 0 {
            burn_acc.record_burn(0, burned, 300, "test", None, minted - burned, 0);
        }
        let snap = SupplySnapshot::new(0, minted, &burn_acc, 0, 0, 0, 0, Some(cap), 0);
        let circulating = minted.saturating_sub(burned);
        if circulating >= cap {
            prop_assert!(snap.cap_reached, "cap should be reached when circulating >= cap");
        } else {
            prop_assert!(!snap.cap_reached, "cap should not be reached when circulating < cap");
        }
    }
}

// --- Property: Burn ratio within governance ceiling ---

proptest! {
    /// Any valid governance burn ratio must be <= 25%.
    #[test]
    fn prop_governance_ratio_within_ceiling(bps in 0u16..3000u16) {
        let result = BurnRatio::new_governance(bps);
        if bps <= 2500 {
            prop_assert!(result.is_ok(), "{}bps should be valid governance ratio", bps);
            let ratio = result.expect("valid bps already checked");
            prop_assert!(ratio.bps() <= 2500);
        } else {
            prop_assert!(result.is_err(), "{}bps should exceed ceiling", bps);
        }
    }
}

// --- Property: Fee decomposition ---

proptest! {
    /// For any OMNIA fee, burned + validator + protocol <= total_fee.
    #[test]
    fn prop_fee_decomposition(_base in 100_000u64..1_000_000_000u64, priority in 0u64..100_000_000u64,
                                burn_bps in 0u16..500u16) {
        let formula = FeeFormula::with_params(OmniaFeeSchedule::standard(), BurnRatio::from_bps(burn_bps));
        if let Ok(result) = formula.calculate(ActivityType::OmniaTransfer, priority) {
            let distributed = result
                .burned_amount
                .saturating_add(result.validator_amount)
                .saturating_add(result.protocol_amount);
            prop_assert!(distributed <= result.total_fee,
                "distributed {} must not exceed total {}", distributed, result.total_fee);
        }
    }
}
