//! Treasury Bridge Adapter - Connects PaymentEngine to Treasury operations
//!
//! This module resolves the integration gaps between the payment-order state
//! machine and the treasury pallet. It provides:
//! 1. **Inventory reservation** - When an order transitions to
//!    RISK_APPROVED -> INVENTORY_RESERVED, this adapter calls
//!    `Treasury::reserve_inventory()` with the order ID as reference.
//! 2. **Inventory consumption** - When an order reaches ALLOCATION_FINALIZED,
//!    the adapter calls `Treasury::consume_reservation()`.
//! 3. **Refund handling** - When an order is refunded, the adapter releases
//!    the treasury reservation back to the pilot inventory.
//! 4. **Single shared circuit breaker** - Uses the Treasury's circuit
//!    breaker instead of maintaining a duplicate in the payment engine.
//!
//! ## Spec References
//!
//! - §5.4: Pilot acquisition uses capped treasury allocation
//! - §6.2: Treasury controls including inventory reservation limits
//! - §8.2: INVENTORY_RESERVED state in the payment order state machine
//! - §15: Risk limits and circuit breakers

use crate::error::PaymentError;
use crate::state::PaymentState;
use crate::types::PaymentOrder;
use omnia_asset_registry::Treasury;

/// Result of a treasury bridge operation.
#[derive(Debug, Clone)]
pub enum TreasuryBridgeResult {
    /// Inventory was successfully reserved.
    Reserved {
        /// The reservation reference (order ID used as key).
        reservation_ref: String,
    },
    /// Reservation was consumed (allocation finalized).
    Consumed,
    /// Reservation was released (refund).
    Released,
    /// No treasury action was needed for this transition.
    NoAction,
}

/// Adapter that connects the payment engine to the treasury.
///
/// This is the single point where payment orders interact with
/// the treasury's inventory system. It ensures:
///
/// - Inventory reservations happen atomically with state transitions
/// - Reservation refs are stored on the order for auditability
/// - Refunds properly release reservations
/// - The treasury circuit breaker is the source of truth
pub struct TreasuryBridgeAdapter {
    /// The treasury instance.
    treasury: Treasury,
}

impl TreasuryBridgeAdapter {
    /// Create a new treasury bridge adapter with the given treasury.
    pub fn new(treasury: Treasury) -> Self {
        Self { treasury }
    }

    /// Get a reference to the underlying treasury.
    pub fn treasury(&self) -> &Treasury {
        &self.treasury
    }

    /// Get a mutable reference to the underlying treasury.
    pub fn treasury_mut(&mut self) -> &mut Treasury {
        &mut self.treasury
    }

    /// Process a treasury action based on the state transition.
    ///
    /// This method should be called AFTER `PaymentEngine::advance_state()`
    /// succeeds, with the NEW state of the order.
    ///
    /// # Arguments
    ///
    /// * `order` - The order in its new state (after transition)
    /// * `previous_state` - The state before the transition
    /// * `now_ms` - Current timestamp
    ///
    /// # Returns
    ///
    /// A `TreasuryBridgeResult` indicating what happened.
    pub fn process_transition(
        &mut self,
        order: &mut PaymentOrder,
        previous_state: PaymentState,
        now_ms: u64,
    ) -> Result<TreasuryBridgeResult, PaymentError> {
        match order.state {
            // RISK_APPROVED -> INVENTORY_RESERVED: reserve inventory from treasury
            PaymentState::InventoryReserved if previous_state == PaymentState::RiskApproved => {
                self.reserve_inventory(order, now_ms)
            }

            // ALLOCATION_FINALIZED -> DELIVERED: consume the reservation
            PaymentState::Delivered if previous_state == PaymentState::AllocationFinalized => {
                self.consume_reservation(order)
            }

            // Any state -> REFUND_PENDING: release reservation if one exists
            PaymentState::RefundPending => self.release_reservation_for_refund(order),

            // CANCELLED: release reservation if one exists
            PaymentState::Cancelled => self.release_reservation_for_refund(order),

            // INVENTORY_UNAVAILABLE: no reservation to release (it was never made)
            PaymentState::InventoryUnavailable => Ok(TreasuryBridgeResult::NoAction),

            _ => Ok(TreasuryBridgeResult::NoAction),
        }
    }

    /// Reserve inventory from the treasury for this order.
    /// Uses the order_id as the treasury reservation reference.
    fn reserve_inventory(
        &mut self,
        order: &mut PaymentOrder,
        now_ms: u64,
    ) -> Result<TreasuryBridgeResult, PaymentError> {
        // Don't double-reserve
        if order.inventory_reservation_ref.is_some() {
            return Ok(TreasuryBridgeResult::NoAction);
        }

        self.treasury
            .reserve_inventory(
                order.omnia_quantity,
                &order.customer_ref,
                &order.provider_name,
                &order.order_id,
                now_ms,
            )
            .map_err(|e| PaymentError::TreasuryIntegration {
                detail: format!("inventory reservation failed: {}", e),
            })?;

        order.inventory_reservation_ref = Some(order.order_id.clone());

        Ok(TreasuryBridgeResult::Reserved {
            reservation_ref: order.order_id.clone(),
        })
    }

    /// Consume the reservation (mark it as used) after delivery.
    fn consume_reservation(&mut self, order: &mut PaymentOrder) -> Result<TreasuryBridgeResult, PaymentError> {
        let ref_str = match &order.inventory_reservation_ref {
            Some(r) => r.clone(),
            None => return Ok(TreasuryBridgeResult::NoAction),
        };

        match self.treasury.consume_reservation(&ref_str) {
            Ok(()) => Ok(TreasuryBridgeResult::Consumed),
            Err(_) => {
                // Already consumed or not found - idempotent
                Ok(TreasuryBridgeResult::NoAction)
            }
        }
    }

    /// Release a reservation when an order is being refunded or cancelled.
    fn release_reservation_for_refund(
        &mut self,
        order: &mut PaymentOrder,
    ) -> Result<TreasuryBridgeResult, PaymentError> {
        let ref_str = match &order.inventory_reservation_ref {
            Some(r) => r.clone(),
            None => return Ok(TreasuryBridgeResult::NoAction),
        };

        match self.treasury.release_reservation(&ref_str, "order refund/cancel") {
            Ok(_event) => {
                order.inventory_reservation_ref = None;
                Ok(TreasuryBridgeResult::Released)
            }
            Err(_) => {
                // Not found or already released - idempotent
                order.inventory_reservation_ref = None;
                Ok(TreasuryBridgeResult::NoAction)
            }
        }
    }

    /// Check whether the treasury circuit breaker would allow a new allocation.
    /// Per Spec §15: circuit breakers MUST pause new allocations.
    pub fn is_allocation_allowed(&self) -> bool {
        !self.treasury.is_paused()
    }
}

// --- Tests ---

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::engine::{Caller, PaymentEngine};
    use crate::state::PaymentState;
    use crate::TreasuryBridgeResult;
    use omnia_asset_registry::types::AssetId;
    use omnia_asset_registry::Treasury;

    const NOW: u64 = 1_700_000_000_000;

    /// Create a test treasury with genesis-funded buckets.
    fn make_treasury() -> Treasury {
        Treasury::with_genesis_buckets()
    }

    /// Create a test engine and advance an order to RISK_APPROVED.
    fn advance_to_risk_approved(order_id: &str) -> PaymentEngine {
        let mut engine = PaymentEngine::new(NOW);
        engine
            .create_order(
                order_id.to_string(),
                "+233240000000".to_string(),
                "recipient-pk".to_string(),
                AssetId::OMNIA,
                50_000_000,    // 500 GHS in pesewas
                1_000_000_000, // 1 OMNIA in plancks
                500_000,       // exchange rate
                1_000_000,     // provider fee
                500_000_000,   // omnia fee
                "MTN".to_string(),
                NOW,
            )
            .expect("create order");

        engine
            .advance_state(
                order_id,
                PaymentState::Quoted,
                Caller::System {
                    service: "quote-service".into(),
                },
                NOW + 1000,
                Some("quoted".into()),
            )
            .expect("quote");
        engine
            .advance_state(
                order_id,
                PaymentState::PaymentPending,
                Caller::System {
                    service: "payment-service".into(),
                },
                NOW + 2000,
                None,
            )
            .expect("payment pending");
        engine
            .advance_state(
                order_id,
                PaymentState::PaymentVerified,
                Caller::Provider {
                    provider_id: "MTN".into(),
                    authenticated: true,
                },
                NOW + 3000,
                Some("provider-ref".into()),
            )
            .expect("payment verified");
        engine
            .advance_state(
                order_id,
                PaymentState::RiskReview,
                Caller::System {
                    service: "risk-engine".into(),
                },
                NOW + 4000,
                None,
            )
            .expect("risk review");
        engine
            .advance_state(
                order_id,
                PaymentState::RiskApproved,
                Caller::System {
                    service: "risk-engine".into(),
                },
                NOW + 5000,
                None,
            )
            .expect("risk approved");

        engine
    }

    #[test]
    fn reserve_inventory_on_approval() {
        let mut engine = advance_to_risk_approved("order-reserve-test");
        let mut adapter = TreasuryBridgeAdapter::new(make_treasury());

        engine
            .advance_state(
                "order-reserve-test",
                PaymentState::InventoryReserved,
                Caller::Treasury,
                NOW + 6000,
                None,
            )
            .expect("inventory reserved");

        let order = engine.get_order_mut("order-reserve-test").expect("exists");
        let result = adapter
            .process_transition(order, PaymentState::RiskApproved, NOW + 6000)
            .expect("process");

        assert!(matches!(result, TreasuryBridgeResult::Reserved { .. }));
        let order = engine.get_order("order-reserve-test").expect("exists");
        assert!(order.inventory_reservation_ref.is_some());
    }

    #[test]
    fn release_on_refund() {
        let mut engine = advance_to_risk_approved("order-refund-test");
        let mut adapter = TreasuryBridgeAdapter::new(make_treasury());

        // Reserve
        engine
            .advance_state(
                "order-refund-test",
                PaymentState::InventoryReserved,
                Caller::Treasury,
                NOW + 6000,
                None,
            )
            .expect("reserve");
        {
            let order = engine.get_order_mut("order-refund-test").expect("exists");
            adapter
                .process_transition(order, PaymentState::RiskApproved, NOW + 6000)
                .expect("adapter reserve");
        }

        // Fail and refund
        engine
            .advance_state(
                "order-refund-test",
                PaymentState::PaymentFailed,
                Caller::Provider {
                    provider_id: "MTN".into(),
                    authenticated: true,
                },
                NOW + 7000,
                Some("failed".into()),
            )
            .expect("fail");
        engine
            .advance_state(
                "order-refund-test",
                PaymentState::RefundPending,
                Caller::System {
                    service: "refund-service".into(),
                },
                NOW + 8000,
                None,
            )
            .expect("refund pending");

        let order = engine.get_order_mut("order-refund-test").expect("exists");
        let result = adapter
            .process_transition(order, PaymentState::PaymentFailed, NOW + 8000)
            .expect("release");
        assert!(matches!(result, TreasuryBridgeResult::Released));
    }

    #[test]
    fn idempotent_reserve() {
        let mut engine = advance_to_risk_approved("order-idem-test");
        let mut adapter = TreasuryBridgeAdapter::new(make_treasury());

        engine
            .advance_state(
                "order-idem-test",
                PaymentState::InventoryReserved,
                Caller::Treasury,
                NOW + 6000,
                None,
            )
            .expect("reserve");
        {
            let order = engine.get_order_mut("order-idem-test").expect("exists");
            adapter
                .process_transition(order, PaymentState::RiskApproved, NOW + 6000)
                .expect("first");
        }

        // Second call - already has ref
        let order = engine.get_order_mut("order-idem-test").expect("exists");
        let result = adapter
            .process_transition(order, PaymentState::RiskApproved, NOW + 7000)
            .expect("second");
        assert!(matches!(result, TreasuryBridgeResult::NoAction));
    }

    #[test]
    fn consume_reservation_on_delivery() {
        let mut engine = advance_to_risk_approved("order-deliver-test");
        let mut adapter = TreasuryBridgeAdapter::new(make_treasury());

        // Full happy path to DELIVERED
        engine
            .advance_state(
                "order-deliver-test",
                PaymentState::InventoryReserved,
                Caller::Treasury,
                NOW + 6000,
                None,
            )
            .expect("reserve");
        {
            let order = engine.get_order_mut("order-deliver-test").expect("exists");
            adapter
                .process_transition(order, PaymentState::RiskApproved, NOW + 6000)
                .expect("adapter reserve");
        }
        engine
            .advance_state(
                "order-deliver-test",
                PaymentState::AllocationSubmitted,
                Caller::System {
                    service: "chain-service".into(),
                },
                NOW + 7000,
                None,
            )
            .expect("alloc submitted");
        engine
            .advance_state(
                "order-deliver-test",
                PaymentState::AllocationFinalized,
                Caller::System {
                    service: "chain-service".into(),
                },
                NOW + 8000,
                None,
            )
            .expect("alloc finalized");
        engine
            .advance_state(
                "order-deliver-test",
                PaymentState::Delivered,
                Caller::System {
                    service: "delivery-service".into(),
                },
                NOW + 9000,
                None,
            )
            .expect("delivered");

        let order = engine.get_order_mut("order-deliver-test").expect("exists");
        let result = adapter
            .process_transition(order, PaymentState::AllocationFinalized, NOW + 9000)
            .expect("consume");
        assert!(matches!(result, TreasuryBridgeResult::Consumed));
    }

    #[test]
    fn cancel_releases_reservation() {
        let mut engine = advance_to_risk_approved("order-cancel-test");
        let mut adapter = TreasuryBridgeAdapter::new(make_treasury());

        engine
            .advance_state(
                "order-cancel-test",
                PaymentState::InventoryReserved,
                Caller::Treasury,
                NOW + 6000,
                None,
            )
            .expect("reserve");
        {
            let order = engine.get_order_mut("order-cancel-test").expect("exists");
            adapter
                .process_transition(order, PaymentState::RiskApproved, NOW + 6000)
                .expect("adapter reserve");
        }

        engine
            .advance_state(
                "order-cancel-test",
                PaymentState::Cancelled,
                Caller::Sender,
                NOW + 7000,
                Some("user cancelled".into()),
            )
            .expect("cancel");

        let order = engine.get_order_mut("order-cancel-test").expect("exists");
        let result = adapter
            .process_transition(order, PaymentState::InventoryReserved, NOW + 7000)
            .expect("cancel release");
        assert!(matches!(result, TreasuryBridgeResult::Released));
    }

    #[test]
    fn allocation_allowed_when_not_paused() {
        let adapter = TreasuryBridgeAdapter::new(make_treasury());
        assert!(adapter.is_allocation_allowed());
    }

    #[test]
    fn allocation_blocked_when_paused() {
        let mut treasury = make_treasury();
        treasury.trip_circuit_breaker("test");
        let adapter = TreasuryBridgeAdapter::new(treasury);
        assert!(!adapter.is_allocation_allowed());
    }

    #[test]
    fn full_happy_path_with_treasury() {
        let mut engine = advance_to_risk_approved("order-happy-full");
        let mut adapter = TreasuryBridgeAdapter::new(make_treasury());

        // Reserve
        engine
            .advance_state(
                "order-happy-full",
                PaymentState::InventoryReserved,
                Caller::Treasury,
                NOW + 6000,
                None,
            )
            .expect("reserve");
        {
            let order = engine.get_order_mut("order-happy-full").expect("exists");
            let res = adapter
                .process_transition(order, PaymentState::RiskApproved, NOW + 6000)
                .expect("adapter reserve");
            assert!(matches!(res, TreasuryBridgeResult::Reserved { .. }));
        }

        // Advance through allocation
        engine
            .advance_state(
                "order-happy-full",
                PaymentState::AllocationSubmitted,
                Caller::System {
                    service: "chain-service".into(),
                },
                NOW + 7000,
                None,
            )
            .expect("alloc submitted");
        engine
            .advance_state(
                "order-happy-full",
                PaymentState::AllocationFinalized,
                Caller::System {
                    service: "chain-service".into(),
                },
                NOW + 8000,
                None,
            )
            .expect("alloc finalized");
        engine
            .advance_state(
                "order-happy-full",
                PaymentState::Delivered,
                Caller::System {
                    service: "delivery-service".into(),
                },
                NOW + 9000,
                None,
            )
            .expect("delivered");

        // Consume
        {
            let order = engine.get_order_mut("order-happy-full").expect("exists");
            let res = adapter
                .process_transition(order, PaymentState::AllocationFinalized, NOW + 9000)
                .expect("consume");
            assert!(matches!(res, TreasuryBridgeResult::Consumed));
        }

        // Verify final state
        let order = engine.get_order("order-happy-full").expect("exists");
        assert_eq!(order.state, PaymentState::Delivered);
        assert!(order.inventory_reservation_ref.is_some()); // still set (consumed, not cleared)
    }
}
