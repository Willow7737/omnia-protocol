//! Background payment-order recovery and operational sweep.
//!
//! The worker is intentionally conservative: it only reconstructs active
//! orders from the durable store and records failures. Provider callbacks,
//! refunds, treasury consumption, and chain delivery remain authenticated
//! side effects handled by their dedicated adapters and service routes.

use std::sync::Arc;
use std::time::Duration;

use omnia_payment_order::{recover_order, PaymentStore};

use crate::state::AppState;

/// Summary of one recovery sweep.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Number of active order IDs returned by the store.
    pub active_orders: usize,
    /// Number of active orders successfully reconstructed.
    pub recovered_orders: usize,
    /// Number of orders whose replay failed or whose snapshot was missing.
    pub failed_orders: usize,
}

/// Run one deterministic recovery sweep against a payment store.
pub fn run_recovery_sweep(store: &dyn PaymentStore) -> Result<RecoveryReport, String> {
    let order_ids = store.list_active_orders().map_err(|error| error.to_string())?;
    let mut report = RecoveryReport {
        active_orders: order_ids.len(),
        ..RecoveryReport::default()
    };

    for order_id in order_ids {
        match recover_order(store, &order_id) {
            Ok(Some(_order)) => report.recovered_orders += 1,
            Ok(None) | Err(_) => report.failed_orders += 1,
        }
    }

    Ok(report)
}

/// Spawn the live recovery worker.
///
/// The interval is configured with `OMNIA_PAYMENT_WORKER_INTERVAL_MS` and
/// defaults to 30 seconds. The returned handle is owned by the node runtime
/// and is cancelled when the runtime shuts down.
pub fn spawn(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    let interval_ms = std::env::var("OMNIA_PAYMENT_WORKER_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value >= 1_000)
        .unwrap_or(30_000);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        loop {
            interval.tick().await;
            match run_recovery_sweep(state.payment_store.as_ref()) {
                Ok(report) if report.failed_orders == 0 => {
                    if report.active_orders > 0 {
                        tracing::debug!(
                            active_orders = report.active_orders,
                            recovered_orders = report.recovered_orders,
                            "Payment recovery sweep completed"
                        );
                    }
                }
                Ok(report) => tracing::error!(
                    active_orders = report.active_orders,
                    recovered_orders = report.recovered_orders,
                    failed_orders = report.failed_orders,
                    "Payment recovery sweep found orders requiring operator attention"
                ),
                Err(error) => tracing::error!(error = %error, "Payment recovery sweep failed"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_payment_order::InMemoryPaymentStore;

    #[test]
    fn empty_store_has_empty_report() {
        let store = InMemoryPaymentStore::new();
        assert_eq!(run_recovery_sweep(&store).expect("sweep"), RecoveryReport::default());
    }
}
