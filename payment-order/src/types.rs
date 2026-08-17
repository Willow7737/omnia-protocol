//! Payment order struct and related types — Spec §8.3

use omnia_asset_registry::types::AssetId;

use crate::state::PaymentState;

/// Immutable record of a single state transition.
///
/// Every transition in the state machine produces one of these.
/// The full history of events enables full audit trail reconstruction
/// per Spec §8.3.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StateTransitionEvent {
    /// Unique order identifier.
    pub order_id: String,
    /// State before the transition.
    pub from_state: PaymentState,
    /// State after the transition.
    pub to_state: PaymentState,
    /// Who or what authorized this transition.
    pub actor: TransitionActor,
    /// Monotonic sequence number for this order's event history.
    pub sequence: u64,
    /// Timestamp (ms since epoch).
    pub timestamp_ms: u64,
    /// Optional reason or reference (e.g., provider ref, error detail).
    pub reason: Option<String>,
}

/// Who authorized a state transition.
/// Maps to the "Authorized By" column in Spec §8.2.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransitionActor {
    /// The sender / wallet initiated the order.
    Sender,
    /// The system (quote service, risk engine, chain service, etc.).
    System {
        /// The name of the system service.
        service: String,
    },
    /// Backend verification service.
    Backend {
        /// The name of the backend service.
        service: String,
    },
    /// Treasury service (inventory reservation).
    Treasury,
    /// Manual review by an operations team member.
    ManualReview {
        /// The reviewer's identifier.
        reviewer: String,
    },
    /// An external provider callback.
    Provider {
        /// The provider's identifier.
        provider_id: String,
    },
}

/// Risk decision attached to an order after risk review.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RiskDecision {
    /// Order has not yet been reviewed.
    Pending,
    /// Risk check passed.
    Approved,
    /// Risk check failed.
    Rejected {
        /// The reason for rejection.
        reason: String,
    },
}

/// Refund status tracking.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RefundStatus {
    /// No refund initiated.
    None,
    /// Refund in progress.
    Pending,
    /// Refund completed.
    Completed,
    /// Refund failed (may be retried).
    Failed {
        /// The reason for failure.
        reason: String,
    },
}

/// The full payment order per Spec §8.3.
///
/// Every field here maps directly to a requirement in the specification.
/// This struct is immutable once created — state changes are recorded
/// in the `event_history` vector.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PaymentOrder {
    // ---- Identity ----
    /// Unique order ID (UUID or comparable).
    pub order_id: String,
    /// Customer reference (mobile-money number or wallet ID).
    pub customer_ref: String,
    /// Recipient reference (wallet public key or account ID).
    pub recipient_ref: String,

    // ---- Asset and amounts ----
    /// Asset being allocated (should be OMNIA for Ghana bridge).
    pub asset_id: AssetId,
    /// GHS amount (fiat input, in smallest GHS unit — pesewas).
    pub ghs_amount: u64,
    /// OMNIA quantity to be allocated (in plancks, 10^-9 OMNIA).
    pub omnia_quantity: u64,
    /// Actual GHS amount received from provider (may differ from quoted).
    pub ghs_received: Option<u64>,

    // ---- Quote ----
    /// Exchange rate applied (GHS per OMNIA in fixed-point).
    pub exchange_rate: u64,
    /// Quote timestamp (ms since epoch).
    pub quote_timestamp_ms: u64,
    /// Quote expiration timestamp (ms since epoch).
    pub quote_expiry_ms: u64,

    // ---- Fees ----
    /// Mobile-money provider fee (in GHS pesewas).
    pub provider_fee: u64,
    /// Omnia protocol fee (in OMNIA plancks).
    pub omnia_fee: u64,

    // ---- Provider ----
    /// Mobile-money provider reference (transaction ID from callback).
    pub provider_ref: Option<String>,
    /// Provider name (e.g., "MTN", "Telecel", "AT").
    pub provider_name: String,

    // ---- Recipient ----
    /// Recipient public key for OMNIA delivery.
    pub recipient_public_key: Option<String>,

    // ---- Inventory ----
    /// Treasury inventory reservation reference (if reserved).
    pub inventory_reservation_ref: Option<String>,

    // ---- Risk ----
    /// Risk decision for this order.
    pub risk_decision: RiskDecision,

    // ---- Status ----
    /// Current payment state.
    pub state: PaymentState,
    /// Current refund status.
    pub refund_status: RefundStatus,

    // ---- Allocation ----
    /// On-chain allocation transaction hash (if submitted).
    pub allocation_tx_hash: Option<String>,
    /// Block number where allocation was finalized (if finalized).
    pub allocation_block: Option<u64>,

    // ---- Audit trail ----
    /// Immutable event history. Every state transition appends one entry.
    /// Per Spec §8.3: "Immutable event history."
    pub event_history: Vec<StateTransitionEvent>,

    // ---- Timestamps ----
    /// Order creation timestamp (ms since epoch).
    pub created_at_ms: u64,
    /// Last state transition timestamp (ms since epoch).
    pub updated_at_ms: u64,
}

impl PaymentOrder {
    /// Create a new payment order in `CREATED` state.
    ///
    /// The initial event history contains a single `StateTransitionEvent`
    /// recording the creation.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        order_id: String,
        customer_ref: String,
        recipient_ref: String,
        asset_id: AssetId,
        ghs_amount: u64,
        omnia_quantity: u64,
        exchange_rate: u64,
        quote_timestamp_ms: u64,
        quote_expiry_ms: u64,
        provider_fee: u64,
        omnia_fee: u64,
        provider_name: String,
        now_ms: u64,
    ) -> Self {
        let creation_event = StateTransitionEvent {
            order_id: order_id.clone(),
            from_state: PaymentState::Created,
            to_state: PaymentState::Created,
            actor: TransitionActor::Sender,
            sequence: 0,
            timestamp_ms: now_ms,
            reason: Some("order created".into()),
        };
        Self {
            order_id,
            customer_ref,
            recipient_ref,
            asset_id,
            ghs_amount,
            omnia_quantity,
            ghs_received: None,
            exchange_rate,
            quote_timestamp_ms,
            quote_expiry_ms,
            provider_fee,
            omnia_fee,
            provider_ref: None,
            provider_name,
            recipient_public_key: None,
            inventory_reservation_ref: None,
            risk_decision: RiskDecision::Pending,
            state: PaymentState::Created,
            refund_status: RefundStatus::None,
            allocation_tx_hash: None,
            allocation_block: None,
            event_history: vec![creation_event],
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    /// Return the next event sequence number.
    #[inline]
    pub fn next_sequence(&self) -> u64 {
        self.event_history
            .last()
            .map(|e| e.sequence.saturating_add(1))
            .unwrap_or(0)
    }

    /// Return true if the order is in a terminal state.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Return true if the order has been economically delivered.
    /// Used to enforce Spec §4.4.
    #[inline]
    pub fn is_economically_delivered(&self) -> bool {
        self.state.is_economically_delivered()
    }

    /// Record a state transition in the event history.
    /// Returns the new event.
    pub fn record_transition(
        &mut self,
        to_state: PaymentState,
        actor: TransitionActor,
        now_ms: u64,
        reason: Option<String>,
    ) -> StateTransitionEvent {
        let from_state = self.state;
        let event = StateTransitionEvent {
            order_id: self.order_id.clone(),
            from_state,
            to_state,
            actor,
            sequence: self.next_sequence(),
            timestamp_ms: now_ms,
            reason,
        };
        self.event_history.push(event.clone());
        self.state = to_state;
        self.updated_at_ms = now_ms;
        event
    }

    /// Return true if the quote is still valid at the given time.
    #[inline]
    pub fn is_quote_valid(&self, now_ms: u64) -> bool {
        now_ms < self.quote_expiry_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_asset_registry::types::AssetId;

    fn test_order() -> PaymentOrder {
        PaymentOrder::new(
            "order-001".into(),
            "+233240000000".into(),
            "recipient-pk-123".into(),
            AssetId::OMNIA,
            50_000_000,        // 500 GHS in pesewas
            100_000_000_000,   // 100 OMNIA in plancks
            500_000,           // exchange rate
            1_700_000_000_000, // quote timestamp
            1_700_000_300_000, // quote expiry (5 min)
            1_000_000,         // provider fee (1 GHS)
            500_000_000,       // omnia fee (0.5 OMNIA)
            "MTN".into(),
            1_700_000_000_000,
        )
    }

    #[test]
    fn new_order_is_created_state() {
        let order = test_order();
        assert_eq!(order.state, PaymentState::Created);
        assert!(!order.is_terminal());
        assert!(!order.is_economically_delivered());
        assert_eq!(order.event_history.len(), 1);
        assert_eq!(order.next_sequence(), 1);
    }

    #[test]
    fn quote_validity() {
        let order = test_order();
        // Before expiry
        assert!(order.is_quote_valid(1_700_000_200_000));
        // At expiry (not valid)
        assert!(!order.is_quote_valid(1_700_000_300_000));
        // After expiry
        assert!(!order.is_quote_valid(1_700_000_400_000));
    }

    #[test]
    fn record_transition_updates_state() {
        let mut order = test_order();
        let event = order.record_transition(
            PaymentState::Quoted,
            TransitionActor::System {
                service: "quote-service".into(),
            },
            1_700_000_001_000,
            Some("quote generated".into()),
        );
        assert_eq!(order.state, PaymentState::Quoted);
        assert_eq!(order.event_history.len(), 2);
        assert_eq!(event.sequence, 1);
        assert_eq!(event.from_state, PaymentState::Created);
        assert_eq!(event.to_state, PaymentState::Quoted);
        assert_eq!(order.updated_at_ms, 1_700_000_001_000);
    }

    #[test]
    fn event_history_is_immutable_audit_trail() {
        let mut order = test_order();
        order.record_transition(
            PaymentState::Quoted,
            TransitionActor::System {
                service: "quote".into(),
            },
            1000,
            None,
        );
        order.record_transition(
            PaymentState::PaymentPending,
            TransitionActor::System {
                service: "payment".into(),
            },
            2000,
            None,
        );
        // First event: creation (seq 0)
        assert_eq!(order.event_history[0].sequence, 0);
        assert_eq!(order.event_history[0].to_state, PaymentState::Created);
        // Second event: quoted (seq 1)
        assert_eq!(order.event_history[1].sequence, 1);
        assert_eq!(order.event_history[1].from_state, PaymentState::Created);
        assert_eq!(order.event_history[1].to_state, PaymentState::Quoted);
        // Third event: payment pending (seq 2)
        assert_eq!(order.event_history[2].sequence, 2);
    }
}
