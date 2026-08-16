//! Payment state enum and transition matrix — Spec §8.2
//!
//! 24 states total: 10 happy-path (including CREATED), 14 failure/recovery.
//! ~35 valid transitions. Terminal states: DELIVERED, REFUNDED, CANCELLED.

use serde::{Deserialize, Serialize};

use crate::error::PaymentError;

/// All 24 payment states per Financial Specification §8.2.
///
/// ## Happy path
///
/// ```text
/// CREATED → QUOTED → PAYMENT_PENDING → PAYMENT_VERIFIED →
/// RISK_REVIEW → RISK_APPROVED → INVENTORY_RESERVED →
/// ALLOCATION_SUBMITTED → ALLOCATION_FINALIZED → DELIVERED
/// ```
///
/// ## Terminal (absorbing) states
///
/// `DELIVERED`, `REFUNDED`, `CANCELLED` — no further transitions allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaymentState {
    // ---- Happy path ----
    /// Payment order created. No funds locked yet.
    Created,
    /// Time-limited OMNIA quote generated (rate, amount, fees, expiry).
    Quoted,
    /// Waiting for mobile-money provider callback.
    PaymentPending,
    /// Provider callback received and independently verified server-side.
    PaymentVerified,
    /// Order queued for risk assessment.
    RiskReview,
    /// Risk check passed.
    RiskApproved,
    /// OMNIA reserved from treasury pilot inventory.
    InventoryReserved,
    /// On-chain OMNIA allocation transaction submitted.
    AllocationSubmitted,
    /// On-chain transaction finalized (confirmed in block).
    AllocationFinalized,
    /// OMNIA delivered to recipient wallet. Terminal state.
    Delivered,

    // ---- Failure / Recovery states ----
    /// Quote timed out before payment.
    QuoteExpired,
    /// Provider reported payment failure.
    PaymentFailed,
    /// Provider reversed a previously successful payment.
    PaymentReversed,
    /// Amount received less than quoted.
    PaymentUnderpaid,
    /// Amount received more than quoted.
    PaymentOverpaid,
    /// No provider callback within timeout.
    PaymentTimeout,
    /// Risk engine rejected the order.
    RiskRejected,
    /// Insufficient treasury inventory to fulfill order.
    InventoryUnavailable,
    /// On-chain allocation transaction failed.
    AllocationFailed,
    /// On-chain transaction not finalized within timeout.
    OnChainTimeout,
    /// Transaction status ambiguous (may or may not have been included).
    OnChainUncertain,
    /// Refund initiated, awaiting processing.
    RefundPending,
    /// Funds returned to sender. Terminal state.
    Refunded,
    /// Requires human intervention.
    ManualReview,
    /// Order cancelled. Terminal state.
    Cancelled,
}

impl PaymentState {
    /// Return true if this is a terminal (absorbing) state.
    /// Terminal states accept no further transitions.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            PaymentState::Delivered | PaymentState::Refunded | PaymentState::Cancelled
        )
    }

    /// Return true if this state represents an economically delivered
    /// outcome (OMNIA was transferred to the recipient).
    /// Used to enforce Spec §4.4: no failed or refunded order can
    /// remain economically delivered.
    #[inline]
    pub fn is_economically_delivered(&self) -> bool {
        matches!(self, PaymentState::AllocationFinalized | PaymentState::Delivered)
    }

    /// Return true if funds are held and a refund path should exist.
    /// When true, the only valid exits must eventually lead to REFUNDED.
    #[inline]
    pub fn has_held_funds(&self) -> bool {
        matches!(
            self,
            PaymentState::PaymentVerified
                | PaymentState::PaymentReversed
                | PaymentState::PaymentUnderpaid
                | PaymentState::PaymentOverpaid
                | PaymentState::RiskRejected
                | PaymentState::AllocationFailed
                | PaymentState::OnChainTimeout
                | PaymentState::OnChainUncertain
                | PaymentState::RefundPending
                | PaymentState::ManualReview
                | PaymentState::InventoryUnavailable
                | PaymentState::InventoryReserved
        )
    }

    /// Return the state label string for event logging.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::Quoted => "QUOTED",
            Self::PaymentPending => "PAYMENT_PENDING",
            Self::PaymentVerified => "PAYMENT_VERIFIED",
            Self::RiskReview => "RISK_REVIEW",
            Self::RiskApproved => "RISK_APPROVED",
            Self::InventoryReserved => "INVENTORY_RESERVED",
            Self::AllocationSubmitted => "ALLOCATION_SUBMITTED",
            Self::AllocationFinalized => "ALLOCATION_FINALIZED",
            Self::Delivered => "DELIVERED",
            Self::QuoteExpired => "QUOTE_EXPIRED",
            Self::PaymentFailed => "PAYMENT_FAILED",
            Self::PaymentReversed => "PAYMENT_REVERSED",
            Self::PaymentUnderpaid => "PAYMENT_UNDERPAID",
            Self::PaymentOverpaid => "PAYMENT_OVERPAID",
            Self::PaymentTimeout => "PAYMENT_TIMEOUT",
            Self::RiskRejected => "RISK_REJECTED",
            Self::InventoryUnavailable => "INVENTORY_UNAVAILABLE",
            Self::AllocationFailed => "ALLOCATION_FAILED",
            Self::OnChainTimeout => "ON_CHAIN_TIMEOUT",
            Self::OnChainUncertain => "ON_CHAIN_UNCERTAIN",
            Self::RefundPending => "REFUND_PENDING",
            Self::Refunded => "REFUNDED",
            Self::ManualReview => "MANUAL_REVIEW",
            Self::Cancelled => "CANCELLED",
        }
    }

    /// Validate a state transition from `self` to `next`.
    /// Returns `Ok(())` if the transition is in the valid transition matrix,
    /// or `PaymentError::InvalidTransition` with details.
    pub fn can_transition_to(&self, next: PaymentState) -> Result<(), PaymentError> {
        if self.is_terminal() {
            return Err(PaymentError::TerminalState {
                state: *self,
                attempted: next,
            });
        }
        if self == &next {
            // Idempotent no-op for non-terminal states — allowed per Spec §8.3
            // (duplicate callbacks must be handled idempotently).
            return Ok(());
        }
        match (self, next) {
            // ---- Happy path ----
            (Self::Created, Self::Quoted)
            | (Self::Quoted, Self::PaymentPending)
            | (Self::PaymentPending, Self::PaymentVerified)
            | (Self::PaymentVerified, Self::RiskReview)
            | (Self::RiskReview, Self::RiskApproved)
            | (Self::RiskApproved, Self::InventoryReserved)
            | (Self::InventoryReserved, Self::AllocationSubmitted)
            | (Self::AllocationSubmitted, Self::AllocationFinalized)
            | (Self::AllocationFinalized, Self::Delivered) => Ok(()),

            // ---- Quote failure ----
            (Self::Created, Self::Cancelled)
            | (Self::Quoted, Self::QuoteExpired)
            | (Self::Quoted, Self::Cancelled) => Ok(()),

            // ---- Payment failures ----
            (Self::PaymentPending, Self::PaymentFailed)
            | (Self::PaymentPending, Self::PaymentTimeout)
            | (Self::PaymentVerified, Self::PaymentReversed) => Ok(()),

            // ---- Amount discrepancies ----
            (Self::PaymentVerified, Self::PaymentUnderpaid)
            | (Self::PaymentVerified, Self::PaymentOverpaid)
            | (Self::PaymentUnderpaid, Self::RefundPending)
            | (Self::PaymentUnderpaid, Self::ManualReview)
            | (Self::PaymentOverpaid, Self::RefundPending)
            | (Self::PaymentOverpaid, Self::ManualReview) => Ok(()),

            // ---- Risk ----
            (Self::RiskReview, Self::RiskRejected)
            | (Self::RiskRejected, Self::RefundPending)
            | (Self::RiskRejected, Self::Cancelled) => Ok(()),

            // ---- Inventory ----
            (Self::RiskApproved, Self::InventoryUnavailable)
            | (Self::InventoryUnavailable, Self::RefundPending)
            | (Self::InventoryUnavailable, Self::Cancelled)
            // A reserved order may be cancelled or refunded before allocation.
            | (Self::InventoryReserved, Self::RefundPending)
            | (Self::InventoryReserved, Self::Cancelled) => Ok(()),

            // ---- Allocation failures ----
            (Self::InventoryReserved, Self::AllocationFailed)
            | (Self::AllocationSubmitted, Self::AllocationFailed)
            | (Self::AllocationFailed, Self::RefundPending)
            | (Self::AllocationFailed, Self::InventoryReserved) => Ok(()), // retry

            // ---- On-chain issues ----
            (Self::AllocationSubmitted, Self::OnChainTimeout)
            | (Self::AllocationSubmitted, Self::OnChainUncertain)
            | (Self::OnChainTimeout, Self::ManualReview)
            | (Self::OnChainUncertain, Self::ManualReview) => Ok(()),

            // ---- Refund path ----
            (Self::PaymentFailed, Self::RefundPending)
            | (Self::PaymentReversed, Self::RefundPending)
            | (Self::PaymentTimeout, Self::ManualReview)
            | (Self::RefundPending, Self::Refunded) => Ok(()),

            // ---- Manual review exits ----
            (Self::ManualReview, Self::RefundPending)
            | (Self::ManualReview, Self::Cancelled)
            | (Self::ManualReview, Self::PaymentPending) // retry after investigation
            | (Self::ManualReview, Self::RiskReview) // re-evaluate
            | (Self::ManualReview, Self::InventoryReserved) => Ok(()), // retry after inventory replenished

            _ => Err(PaymentError::InvalidTransition {
                from: *self,
                to: next,
            }),
        }
    }
}

impl std::fmt::Display for PaymentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Happy path: all 9 transitions in sequence
    // ------------------------------------------------------------------

    #[test]
    fn happy_path_all_transitions_valid() {
        let path = [
            (PaymentState::Created, PaymentState::Quoted),
            (PaymentState::Quoted, PaymentState::PaymentPending),
            (PaymentState::PaymentPending, PaymentState::PaymentVerified),
            (PaymentState::PaymentVerified, PaymentState::RiskReview),
            (PaymentState::RiskReview, PaymentState::RiskApproved),
            (PaymentState::RiskApproved, PaymentState::InventoryReserved),
            (PaymentState::InventoryReserved, PaymentState::AllocationSubmitted),
            (PaymentState::AllocationSubmitted, PaymentState::AllocationFinalized),
            (PaymentState::AllocationFinalized, PaymentState::Delivered),
        ];
        for (from, to) in &path {
            assert!(
                from.can_transition_to(*to).is_ok(),
                "expected valid transition: {from} → {to}"
            );
        }
    }

    // ------------------------------------------------------------------
    // Terminal states: no transitions allowed (except self-idempotent)
    // ------------------------------------------------------------------

    #[test]
    fn terminal_states_reject_all_transitions() {
        let terminals = [PaymentState::Delivered, PaymentState::Refunded, PaymentState::Cancelled];
        let targets = [
            PaymentState::Created,
            PaymentState::Quoted,
            PaymentState::RefundPending,
            PaymentState::ManualReview,
        ];
        for terminal in &terminals {
            assert!(terminal.is_terminal());
            for target in &targets {
                let result = terminal.can_transition_to(*target);
                assert!(
                    result.is_err(),
                    "terminal {terminal} should reject transition to {target}"
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Idempotent self-transitions on non-terminal states
    // ------------------------------------------------------------------

    #[test]
    fn idempotent_self_transition_allowed() {
        let non_terminal = [
            PaymentState::Created,
            PaymentState::Quoted,
            PaymentState::PaymentPending,
            PaymentState::PaymentVerified,
            PaymentState::RiskReview,
            PaymentState::RiskApproved,
            PaymentState::InventoryReserved,
            PaymentState::AllocationSubmitted,
            PaymentState::AllocationFinalized,
            PaymentState::ManualReview,
            PaymentState::RefundPending,
        ];
        for state in &non_terminal {
            assert!(!state.is_terminal());
            assert!(
                state.can_transition_to(*state).is_ok(),
                "non-terminal {state} should allow idempotent self-transition"
            );
        }
    }

    // ------------------------------------------------------------------
    // Invalid transitions
    // ------------------------------------------------------------------

    #[test]
    fn reject_skip_ahead() {
        // Created → PaymentPending skips QUOTED
        assert!(PaymentState::Created
            .can_transition_to(PaymentState::PaymentPending)
            .is_err());
        // Created → Delivered skips everything
        assert!(PaymentState::Created
            .can_transition_to(PaymentState::Delivered)
            .is_err());
    }

    #[test]
    fn reject_backward_transition() {
        // PaymentVerified → Quoted (going backward)
        assert!(PaymentState::PaymentVerified
            .can_transition_to(PaymentState::Quoted)
            .is_err());
        // AllocationFinalized → RiskReview (going backward)
        assert!(PaymentState::AllocationFinalized
            .can_transition_to(PaymentState::RiskReview)
            .is_err());
    }

    // ------------------------------------------------------------------
    // Failure paths
    // ------------------------------------------------------------------

    #[test]
    fn quote_failure_paths() {
        assert!(PaymentState::Created.can_transition_to(PaymentState::Cancelled).is_ok());
        assert!(PaymentState::Quoted
            .can_transition_to(PaymentState::QuoteExpired)
            .is_ok());
        assert!(PaymentState::Quoted.can_transition_to(PaymentState::Cancelled).is_ok());
    }

    #[test]
    fn payment_failure_paths() {
        assert!(PaymentState::PaymentPending
            .can_transition_to(PaymentState::PaymentFailed)
            .is_ok());
        assert!(PaymentState::PaymentPending
            .can_transition_to(PaymentState::PaymentTimeout)
            .is_ok());
        assert!(PaymentState::PaymentVerified
            .can_transition_to(PaymentState::PaymentReversed)
            .is_ok());
    }

    #[test]
    fn amount_discrepancy_paths() {
        assert!(PaymentState::PaymentVerified
            .can_transition_to(PaymentState::PaymentUnderpaid)
            .is_ok());
        assert!(PaymentState::PaymentVerified
            .can_transition_to(PaymentState::PaymentOverpaid)
            .is_ok());
        assert!(PaymentState::PaymentUnderpaid
            .can_transition_to(PaymentState::RefundPending)
            .is_ok());
        assert!(PaymentState::PaymentUnderpaid
            .can_transition_to(PaymentState::ManualReview)
            .is_ok());
        assert!(PaymentState::PaymentOverpaid
            .can_transition_to(PaymentState::RefundPending)
            .is_ok());
        assert!(PaymentState::PaymentOverpaid
            .can_transition_to(PaymentState::ManualReview)
            .is_ok());
    }

    #[test]
    fn risk_paths() {
        assert!(PaymentState::RiskReview
            .can_transition_to(PaymentState::RiskRejected)
            .is_ok());
        assert!(PaymentState::RiskRejected
            .can_transition_to(PaymentState::RefundPending)
            .is_ok());
        assert!(PaymentState::RiskRejected
            .can_transition_to(PaymentState::Cancelled)
            .is_ok());
    }

    #[test]
    fn inventory_paths() {
        assert!(PaymentState::RiskApproved
            .can_transition_to(PaymentState::InventoryUnavailable)
            .is_ok());
        assert!(PaymentState::InventoryUnavailable
            .can_transition_to(PaymentState::RefundPending)
            .is_ok());
        assert!(PaymentState::InventoryUnavailable
            .can_transition_to(PaymentState::Cancelled)
            .is_ok());
    }

    #[test]
    fn allocation_failure_paths() {
        assert!(PaymentState::InventoryReserved
            .can_transition_to(PaymentState::AllocationFailed)
            .is_ok());
        assert!(PaymentState::AllocationSubmitted
            .can_transition_to(PaymentState::AllocationFailed)
            .is_ok());
        assert!(PaymentState::AllocationFailed
            .can_transition_to(PaymentState::RefundPending)
            .is_ok());
        // Retry: ALLOCATION_FAILED → INVENTORY_RESERVED
        assert!(PaymentState::AllocationFailed
            .can_transition_to(PaymentState::InventoryReserved)
            .is_ok());
    }

    #[test]
    fn on_chain_failure_paths() {
        assert!(PaymentState::AllocationSubmitted
            .can_transition_to(PaymentState::OnChainTimeout)
            .is_ok());
        assert!(PaymentState::AllocationSubmitted
            .can_transition_to(PaymentState::OnChainUncertain)
            .is_ok());
        assert!(PaymentState::OnChainTimeout
            .can_transition_to(PaymentState::ManualReview)
            .is_ok());
        assert!(PaymentState::OnChainUncertain
            .can_transition_to(PaymentState::ManualReview)
            .is_ok());
    }

    #[test]
    fn refund_paths() {
        assert!(PaymentState::PaymentFailed
            .can_transition_to(PaymentState::RefundPending)
            .is_ok());
        assert!(PaymentState::PaymentReversed
            .can_transition_to(PaymentState::RefundPending)
            .is_ok());
        assert!(PaymentState::PaymentTimeout
            .can_transition_to(PaymentState::ManualReview)
            .is_ok());
        assert!(PaymentState::RefundPending
            .can_transition_to(PaymentState::Refunded)
            .is_ok());
    }

    #[test]
    fn manual_review_exit_paths() {
        assert!(PaymentState::ManualReview
            .can_transition_to(PaymentState::RefundPending)
            .is_ok());
        assert!(PaymentState::ManualReview
            .can_transition_to(PaymentState::Cancelled)
            .is_ok());
        assert!(PaymentState::ManualReview
            .can_transition_to(PaymentState::PaymentPending)
            .is_ok());
        assert!(PaymentState::ManualReview
            .can_transition_to(PaymentState::RiskReview)
            .is_ok());
        assert!(PaymentState::ManualReview
            .can_transition_to(PaymentState::InventoryReserved)
            .is_ok());
    }

    // ------------------------------------------------------------------
    // Economic delivery invariant
    // ------------------------------------------------------------------

    #[test]
    fn only_two_states_are_economically_delivered() {
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
        let delivered: Vec<_> = all_states
            .iter()
            .filter(|s| s.is_economically_delivered())
            .copied()
            .collect();
        assert_eq!(
            delivered,
            vec![PaymentState::AllocationFinalized, PaymentState::Delivered],
            "only ALLOCATION_FINALIZED and DELIVERED should be economically delivered"
        );
    }

    // ------------------------------------------------------------------
    // Count: 24 states, 3 terminals
    // ------------------------------------------------------------------

    #[test]
    fn state_count_matches_adr() {
        let all = [
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
        // 10 happy-path + 15 failure/recovery = 25 states (ADR-028 lists all 25)
        assert_eq!(all.len(), 25, "must have 25 payment states (10 happy + 15 failure)");
        let terminals: Vec<_> = all.iter().filter(|s| s.is_terminal()).collect();
        assert_eq!(terminals.len(), 3, "must have exactly 3 terminal states");
    }

    // ------------------------------------------------------------------
    // Labels match expected strings
    // ------------------------------------------------------------------

    #[test]
    fn labels_are_upper_snake_case() {
        assert_eq!(PaymentState::Created.label(), "CREATED");
        assert_eq!(PaymentState::PaymentPending.label(), "PAYMENT_PENDING");
        assert_eq!(PaymentState::AllocationFinalized.label(), "ALLOCATION_FINALIZED");
        assert_eq!(PaymentState::OnChainUncertain.label(), "ON_CHAIN_UNCERTAIN");
        assert_eq!(PaymentState::Delivered.label(), "DELIVERED");
    }
}
