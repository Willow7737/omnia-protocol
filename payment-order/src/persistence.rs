//! Durable persistence for PaymentEngine — Audit Priority 6
//!
//! Per the Omnia Checkpoint Audit:
//! "A process restart must not lose a reservation, duplicate a refund,
//!  or deliver OMNIA twice."
//!
//! This module provides an event-sourced persistence layer:
//! 1. Every state transition is appended to an append-only event log
//! 2. On recovery, the engine replays events to reconstruct state
//! 3. Idempotency keys prevent duplicate processing
//! 4. An outbox pattern tracks pending side effects (treasury ops,
//!    provider calls)

use std::collections::{HashMap, HashSet};

use crate::error::PaymentError;
use crate::types::{PaymentOrder, StateTransitionEvent};

/// Trait for durable event storage.
/// Implementors persist events to disk, database, or distributed log.
pub trait PaymentStore: Send + Sync {
    /// Append a transition event. Must fail if the event already exists
    /// (by order_id + sequence) — enabling idempotent replay.
    fn append_event(&self, event: &StateTransitionEvent) -> Result<(), PaymentError>;

    /// Load all events for an order, in sequence order.
    fn load_events(&self, order_id: &str) -> Result<Vec<StateTransitionEvent>, PaymentError>;

    /// Mark a side-effect as completed (e.g., treasury reservation made).
    fn mark_side_effect_done(&self, order_id: &str, effect_type: &str) -> Result<(), PaymentError>;

    /// Check if a side-effect has been completed.
    fn is_side_effect_done(&self, order_id: &str, effect_type: &str) -> Result<bool, PaymentError>;

    /// List all non-terminal order IDs.
    fn list_active_orders(&self) -> Result<Vec<String>, PaymentError>;

    /// Persist the full order snapshot (for fast recovery without replay).
    fn save_order_snapshot(&self, order: &PaymentOrder) -> Result<(), PaymentError>;

    /// Load an order snapshot. Returns None if no snapshot exists.
    fn load_order_snapshot(&self, order_id: &str) -> Result<Option<PaymentOrder>, PaymentError>;
}

/// In-memory implementation for testing. NOT production-durable.
#[derive(Debug, Default)]
pub struct InMemoryPaymentStore {
    events: std::sync::Mutex<
        HashMap<String, Vec<StateTransitionEvent>>, // order_id -> events
    >,
    side_effects: std::sync::Mutex<
        HashMap<String, HashSet<String>>, // order_id -> set of completed effect types
    >,
    order_snapshots: std::sync::Mutex<
        HashMap<String, PaymentOrder>,
    >,
}

impl InMemoryPaymentStore {
    /// Create a new in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl PaymentStore for InMemoryPaymentStore {
    fn append_event(&self, event: &StateTransitionEvent) -> Result<(), PaymentError> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| PaymentError::PersistenceError("lock poisoned".into()))?;
        let order_events = events.entry(event.order_id.clone()).or_default();

        // Idempotency: if we already have this sequence, skip
        if order_events
            .iter()
            .any(|e| e.sequence == event.sequence)
        {
            return Ok(());
        }

        order_events.push(event.clone());
        Ok(())
    }

    fn load_events(&self, order_id: &str) -> Result<Vec<StateTransitionEvent>, PaymentError> {
        let events = self
            .events
            .lock()
            .map_err(|_| PaymentError::PersistenceError("lock poisoned".into()))?;
        Ok(events.get(order_id).cloned().unwrap_or_default())
    }

    fn mark_side_effect_done(&self, order_id: &str, effect_type: &str) -> Result<(), PaymentError> {
        let mut effects = self
            .side_effects
            .lock()
            .map_err(|_| PaymentError::PersistenceError("lock poisoned".into()))?;
        effects
            .entry(order_id.to_string())
            .or_default()
            .insert(effect_type.to_string());
        Ok(())
    }

    fn is_side_effect_done(&self, order_id: &str, effect_type: &str) -> Result<bool, PaymentError> {
        let effects = self
            .side_effects
            .lock()
            .map_err(|_| PaymentError::PersistenceError("lock poisoned".into()))?;
        Ok(effects
            .get(order_id)
            .is_some_and(|set| set.contains(effect_type)))
    }

    fn list_active_orders(&self) -> Result<Vec<String>, PaymentError> {
        let snapshots = self
            .order_snapshots
            .lock()
            .map_err(|_| PaymentError::PersistenceError("lock poisoned".into()))?;
        Ok(snapshots
            .iter()
            .filter(|(_, order)| !order.is_terminal())
            .map(|(id, _)| id.clone())
            .collect())
    }

    fn save_order_snapshot(&self, order: &PaymentOrder) -> Result<(), PaymentError> {
        let mut snapshots = self
            .order_snapshots
            .lock()
            .map_err(|_| PaymentError::PersistenceError("lock poisoned".into()))?;
        snapshots.insert(order.order_id.clone(), order.clone());
        Ok(())
    }

    fn load_order_snapshot(&self, order_id: &str) -> Result<Option<PaymentOrder>, PaymentError> {
        let snapshots = self
            .order_snapshots
            .lock()
            .map_err(|_| PaymentError::PersistenceError("lock poisoned".into()))?;
        Ok(snapshots.get(order_id).cloned())
    }
}

/// Recover a `PaymentOrder` from stored events by replaying them.
/// If a snapshot exists, it is used as the base and only newer events
/// are replayed on top.
pub fn recover_order(
    store: &dyn PaymentStore,
    order_id: &str,
) -> Result<Option<PaymentOrder>, PaymentError> {
    // Try snapshot first
    let base = store.load_order_snapshot(order_id)?;
    let events = store.load_events(order_id)?;

    if events.is_empty() {
        return Ok(base);
    }

    // Determine the starting point
    let (mut order, start_seq) = if let Some(snapshot) = base {
        let last_seq = snapshot
            .event_history
            .last()
            .map(|e| e.sequence)
            .unwrap_or(0);
        (snapshot, last_seq + 1)
    } else {
        // Reconstruct from first event (creation)
        let creation = &events[0];
        let order = PaymentOrder::new(
            creation.order_id.clone(),
            String::new(), // customer_ref — not in event
            String::new(), // recipient_ref — not in event
            omnia_asset_registry::types::AssetId::OMNIA,
            0, // ghs_amount — not in event
            0, // omnia_quantity — not in event
            0, // exchange_rate
            0, // quote_timestamp_ms
            0, // quote_expiry_ms
            0, // provider_fee
            0, // omnia_fee
            String::new(), // provider_name
            creation.timestamp_ms,
        );
        (order, 1)
    };

    // Replay events starting from start_seq
    for event in &events {
        if event.sequence < start_seq {
            continue;
        }
        order.state = event.to_state;
        order.updated_at_ms = event.timestamp_ms;
        order.event_history.push(event.clone());
    }

    Ok(Some(order))
}

/// Side effect types tracked in the outbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SideEffect {
    /// Treasury inventory reservation.
    TreasuryReserve,
    /// Treasury inventory consumption (delivery).
    TreasuryConsume,
    /// Treasury inventory release (refund/cancel).
    TreasuryRelease,
    /// Provider payment initiation.
    ProviderInitiatePayment,
    /// Provider refund initiation.
    ProviderRefund,
    /// On-chain allocation submission.
    ChainAllocation,
    /// Delivery notification to wallet.
    DeliveryNotify,
}

impl SideEffect {
    /// String key for persistence.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TreasuryReserve => "treasury_reserve",
            Self::TreasuryConsume => "treasury_consume",
            Self::TreasuryRelease => "treasury_release",
            Self::ProviderInitiatePayment => "provider_initiate_payment",
            Self::ProviderRefund => "provider_refund",
            Self::ChainAllocation => "chain_allocation",
            Self::DeliveryNotify => "delivery_notify",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::PaymentState;
    use crate::types::TransitionActor;
    use omnia_asset_registry::types::AssetId;

    const NOW: u64 = 1_700_000_000_000;

    fn make_test_order() -> PaymentOrder {
        PaymentOrder::new(
            "order-persist-1".into(),
            "+233240000000".into(),
            "recipient-pk".into(),
            AssetId::OMNIA,
            100_000,
            200_000_000_000,
            500_000,
            NOW,
            NOW + 300_000,
            1_000,
            500_000_000,
            "MTN".into(),
            NOW,
        )
    }

    #[test]
    fn in_memory_store_append_and_load() {
        let store = InMemoryPaymentStore::new();
        let order = make_test_order();
        let event = &order.event_history[0];

        store.append_event(event).expect("append");
        let loaded = store.load_events("order-persist-1").expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].order_id, "order-persist-1");
    }

    #[test]
    fn in_memory_store_idempotent_append() {
        let store = InMemoryPaymentStore::new();
        let order = make_test_order();
        let event = &order.event_history[0];

        store.append_event(event).expect("first");
        store.append_event(event).expect("second"); // duplicate
        let loaded = store.load_events("order-persist-1").expect("load");
        assert_eq!(loaded.len(), 1); // still just one
    }

    #[test]
    fn in_memory_store_side_effects() {
        let store = InMemoryPaymentStore::new();

        assert!(!store.is_side_effect_done("o1", "treasury_reserve").expect("check"));

        store
            .mark_side_effect_done("o1", "treasury_reserve")
            .expect("mark");
        assert!(store.is_side_effect_done("o1", "treasury_reserve").expect("check"));
        assert!(!store.is_side_effect_done("o1", "treasury_consume").expect("check"));
    }

    #[test]
    fn in_memory_store_snapshot() {
        let store = InMemoryPaymentStore::new();
        let order = make_test_order();

        assert!(store
            .load_order_snapshot("order-persist-1")
            .expect("load")
            .is_none());

        store
            .save_order_snapshot(&order)
            .expect("save");
        let loaded = store
            .load_order_snapshot("order-persist-1")
            .expect("load")
            .expect("should exist");
        assert_eq!(loaded.order_id, "order-persist-1");
    }

    #[test]
    fn in_memory_store_list_active_orders() {
        let store = InMemoryPaymentStore::new();
        let order = make_test_order();
        store.save_order_snapshot(&order).expect("save");

        let active = store.list_active_orders().expect("list");
        assert!(active.contains(&"order-persist-1".to_string()));
    }

    #[test]
    fn recover_order_from_events() {
        let store = InMemoryPaymentStore::new();
        let mut order = make_test_order();

        // Simulate two transitions
        let ev1 = order.record_transition(
            PaymentState::Quoted,
            TransitionActor::System {
                service: "quote-service".into(),
            },
            NOW + 1000,
            Some("quoted".into()),
        );
        store.append_event(&ev1).expect("append ev1");

        let ev2 = order.record_transition(
            PaymentState::PaymentPending,
            TransitionActor::System {
                service: "payment-service".into(),
            },
            NOW + 2000,
            None,
        );
        store.append_event(&ev2).expect("append ev2");

        // Recover
        let recovered = recover_order(&store, "order-persist-1")
            .expect("recover")
            .expect("should exist");
        assert_eq!(recovered.state, PaymentState::PaymentPending);
        assert_eq!(recovered.event_history.len(), 3); // creation + 2 transitions
    }

    #[test]
    fn side_effect_as_str() {
        assert_eq!(SideEffect::TreasuryReserve.as_str(), "treasury_reserve");
        assert_eq!(SideEffect::ProviderRefund.as_str(), "provider_refund");
    }
}
