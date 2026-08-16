//! Reservation-release authorization tests — Audit Priority 2
//!
//! Tests the fix from commit d0b0fa9:
//! INVENTORY_RESERVED → REFUND_PENDING / CANCELLED transitions.
//!
//! Per the audit:
//! - `system:refund-service` may move a reserved order to `REFUND_PENDING`.
//! - `system` or the sender may move a reserved order to `CANCELLED`.
//! - Unauthorized callers must be rejected.

use omnia_payment_order::engine::Caller;
use omnia_payment_order::risk::RiskLimits;
use omnia_payment_order::state::PaymentState;
use omnia_payment_order::types::{PaymentOrder, RefundStatus, StateTransitionEvent, TransitionActor};
use omnia_payment_order::PaymentEngine;
use omnia_payment_order::PaymentError;
use omnia_asset_registry::types::AssetId;

const NOW: u64 = 1_700_000_000_000;

/// Create a test engine with raised per-order limits.
fn make_engine() -> PaymentEngine {
    let limits = RiskLimits {
        per_order_ghs_limit: 100_000_000,
        ..RiskLimits::default()
    };
    PaymentEngine::with_limits(limits, NOW)
}

/// Advance an order to INVENTORY_RESERVED state.
fn advance_to_inventory_reserved(engine: &mut PaymentEngine, order_id: &str) {
    engine
        .create_order(
            order_id.to_string(),
            "+233240000000".to_string(),
            "recipient-pk".to_string(),
            AssetId::OMNIA,
            50_000_000,
            1_000_000_000,
            500_000,
            1_000,
            500_000_000,
            "MTN".to_string(),
            NOW,
        )
        .expect("create order");

    engine
        .advance_state(
            order_id,
            PaymentState::Quoted,
            Caller::System { service: "quote-service".into() },
            NOW + 1000,
            Some("quoted".into()),
        )
        .expect("quote");
    engine
        .advance_state(
            order_id,
            PaymentState::PaymentPending,
            Caller::System { service: "payment-service".into() },
            NOW + 2000,
            None,
        )
        .expect("payment pending");
    engine
        .advance_state(
            order_id,
            PaymentState::PaymentVerified,
            Caller::Provider { provider_id: "MTN".into(), authenticated: true },
            NOW + 3000,
            Some("provider-ref".into()),
        )
        .expect("payment verified");
    engine
        .advance_state(
            order_id,
            PaymentState::RiskReview,
            Caller::System { service: "risk-engine".into() },
            NOW + 4000,
            None,
        )
        .expect("risk review");
    engine
        .advance_state(
            order_id,
            PaymentState::RiskApproved,
            Caller::System { service: "risk-engine".into() },
            NOW + 5000,
            None,
        )
        .expect("risk approved");
    engine
        .advance_state(
            order_id,
            PaymentState::InventoryReserved,
            Caller::Treasury,
            NOW + 6000,
            None,
        )
        .expect("inventory reserved");
}

// ------------------------------------------------------------------
// Allowed transitions from INVENTORY_RESERVED
// ------------------------------------------------------------------

#[test]
fn reserved_to_refund_pending_by_refund_service() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-rp-1");

    engine
        .advance_state(
            "order-rp-1",
            PaymentState::RefundPending,
            Caller::System { service: "refund-service".into() },
            NOW + 7000,
            Some("customer requested refund".into()),
        )
        .expect("refund pending should succeed");

    let order = engine.get_order("order-rp-1").expect("exists");
    assert_eq!(order.state, PaymentState::RefundPending);
    assert_eq!(order.refund_status, RefundStatus::Pending);
}

#[test]
fn reserved_to_cancelled_by_system() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-cs-1");

    engine
        .advance_state(
            "order-cs-1",
            PaymentState::Cancelled,
            Caller::System { service: "payment-service".into() },
            NOW + 7000,
            Some("system cancelled".into()),
        )
        .expect("cancel by system should succeed");

    let order = engine.get_order("order-cs-1").expect("exists");
    assert_eq!(order.state, PaymentState::Cancelled);
}

#[test]
fn reserved_to_cancelled_by_sender() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-sc-1");

    engine
        .advance_state(
            "order-sc-1",
            PaymentState::Cancelled,
            Caller::Sender,
            NOW + 7000,
            Some("user cancelled".into()),
        )
        .expect("cancel by sender should succeed");

    let order = engine.get_order("order-sc-1").expect("exists");
    assert_eq!(order.state, PaymentState::Cancelled);
}

#[test]
fn reserved_to_allocation_submitted_by_chain_service() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-as-1");

    engine
        .advance_state(
            "order-as-1",
            PaymentState::AllocationSubmitted,
            Caller::System { service: "chain-service".into() },
            NOW + 7000,
            None,
        )
        .expect("allocation submitted should succeed");

    let order = engine.get_order("order-as-1").expect("exists");
    assert_eq!(order.state, PaymentState::AllocationSubmitted);
}

#[test]
fn reserved_to_allocation_failed_by_chain_service() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-af-1");

    engine
        .advance_state(
            "order-af-1",
            PaymentState::AllocationFailed,
            Caller::System { service: "chain-service".into() },
            NOW + 7000,
            Some("gas estimation failed".into()),
        )
        .expect("allocation failed should succeed");

    let order = engine.get_order("order-af-1").expect("exists");
    assert_eq!(order.state, PaymentState::AllocationFailed);
}

// ------------------------------------------------------------------
// Rejected transitions from INVENTORY_RESERVED
// ------------------------------------------------------------------

#[test]
fn reserved_to_refund_pending_rejected_for_wrong_service() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-rp-rej-1");

    let err = engine
        .advance_state(
            "order-rp-rej-1",
            PaymentState::RefundPending,
            Caller::System { service: "risk-engine".into() },
            NOW + 7000,
            None,
        )
        .expect_err("should reject wrong service");

    assert!(matches!(err, PaymentError::Unauthorized { .. }));
}

#[test]
fn reserved_to_refund_pending_rejected_for_sender() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-rp-rej-2");

    let err = engine
        .advance_state(
            "order-rp-rej-2",
            PaymentState::RefundPending,
            Caller::Sender,
            NOW + 7000,
            None,
        )
        .expect_err("should reject sender");

    assert!(matches!(err, PaymentError::Unauthorized { .. }));
}

#[test]
fn reserved_to_refund_pending_rejected_for_unauthenticated_provider() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-rp-rej-3");

    let err = engine
        .advance_state(
            "order-rp-rej-3",
            PaymentState::RefundPending,
            Caller::Provider { provider_id: "MTN".into(), authenticated: false },
            NOW + 7000,
            None,
        )
        .expect_err("should reject unauthenticated provider");

    assert!(matches!(err, PaymentError::Unauthorized { .. }));
}

#[test]
fn reserved_to_cancelled_rejected_for_provider() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-c-rej-1");

    let err = engine
        .advance_state(
            "order-c-rej-1",
            PaymentState::Cancelled,
            Caller::Provider { provider_id: "MTN".into(), authenticated: true },
            NOW + 7000,
            None,
        )
        .expect_err("should reject provider");

    assert!(matches!(err, PaymentError::Unauthorized { .. }));
}

#[test]
fn reserved_to_cancelled_rejected_for_treasury() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-c-rej-2");

    let err = engine
        .advance_state(
            "order-c-rej-2",
            PaymentState::Cancelled,
            Caller::Treasury,
            NOW + 7000,
            None,
        )
        .expect_err("should reject treasury");

    assert!(matches!(err, PaymentError::Unauthorized { .. }));
}

#[test]
fn reserved_to_delivered_rejected() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-d-rej-1");

    let err = engine
        .advance_state(
            "order-d-rej-1",
            PaymentState::Delivered,
            Caller::System { service: "delivery-service".into() },
            NOW + 7000,
            None,
        )
        .expect_err("should reject skip");

    assert!(matches!(err, PaymentError::InvalidTransition { .. }));
}

// ------------------------------------------------------------------
// Full refund path from INVENTORY_RESERVED
// ------------------------------------------------------------------

#[test]
fn full_refund_path_from_reserved() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-full-rf-1");

    engine
        .advance_state(
            "order-full-rf-1",
            PaymentState::RefundPending,
            Caller::System { service: "refund-service".into() },
            NOW + 7000,
            Some("customer requested".into()),
        )
        .expect("refund pending");

    engine
        .advance_state(
            "order-full-rf-1",
            PaymentState::Refunded,
            Caller::System { service: "refund-service".into() },
            NOW + 8000,
            None,
        )
        .expect("refunded");

    let order = engine.get_order("order-full-rf-1").expect("exists");
    assert_eq!(order.state, PaymentState::Refunded);
    assert!(order.is_terminal());
    assert_eq!(order.refund_status, RefundStatus::Completed);
}

// ------------------------------------------------------------------
// Event history audit trail
// ------------------------------------------------------------------

#[test]
fn refund_from_reserved_has_correct_audit_trail() {
    let mut engine = make_engine();
    advance_to_inventory_reserved(&mut engine, "order-audit-1");

    engine
        .advance_state(
            "order-audit-1",
            PaymentState::RefundPending,
            Caller::System { service: "refund-service".into() },
            NOW + 7000,
            Some("test refund".into()),
        )
        .expect("refund pending");

    let order = engine.get_order("order-audit-1").expect("exists");

    // creation + 6 happy-path transitions + 1 refund = 8 events
    assert_eq!(order.event_history.len(), 8);

    let last = &order.event_history[7];
    assert_eq!(last.from_state, PaymentState::InventoryReserved);
    assert_eq!(last.to_state, PaymentState::RefundPending);
    assert_eq!(last.sequence, 7);
    assert_eq!(
        last.actor,
        TransitionActor::System { service: "refund-service".into() }
    );
}