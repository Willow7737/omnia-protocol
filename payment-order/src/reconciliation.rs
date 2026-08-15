//! Financial reconciliation system — Spec §14
//!
//! ## Requirements (§14)
//!
//! - Double-entry-style operational ledger
//! - 6-way reconciliation chain: provider ↔ orders ↔ inventory ↔ allocation ↔ wallet ↔ merchant ↔ refunds
//! - 10 daily controls
//! - No silent balance discrepancy write-offs

use serde::{Deserialize, Serialize};

use crate::state::PaymentState;

// --- Reconciliation Entry ---

/// A single entry in the operational ledger.
/// Double-entry: every debit has a corresponding credit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// Unique entry ID.
    pub entry_id: String,
    /// The order or reference this entry relates to.
    pub reference: String,
    /// Debit account (source).
    pub debit_account: String,
    /// Credit account (destination).
    pub credit_account: String,
    /// Amount in smallest unit.
    pub amount: u64,
    /// Asset ID.
    pub asset_id: u32,
    /// Entry type.
    pub entry_type: LedgerEntryType,
    /// Timestamp (ms).
    pub timestamp_ms: u64,
    /// Sequence for ordering.
    pub sequence: u64,
}

/// Type of ledger entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerEntryType {
    /// GHS received from customer (mobile-money provider).
    GhsReceived,
    /// OMNIA allocated from treasury inventory.
    OmniaAllocated,
    /// OMNIA delivered to recipient wallet.
    OmniaDelivered,
    /// GHS refunded to customer.
    GhsRefunded,
    /// OMNIA returned to treasury (allocation reversal).
    OmniaReturned,
    /// Provider fee paid.
    ProviderFeePaid,
    /// Protocol fee collected.
    ProtocolFeeCollected,
    /// Fee burned.
    FeeBurned,
    /// Correction/adjustment entry.
    Correction,
}

// --- Discrepancy ---

/// A discrepancy found during reconciliation.
/// Per Spec §14: "No silent balance discrepancy write-offs;
/// every difference needs owner, reason, status, resolution."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discrepancy {
    /// Unique discrepancy ID.
    pub discrepancy_id: String,
    /// The reconciliation check that found this.
    pub check_type: ReconciliationCheck,
    /// Human-readable description.
    pub description: String,
    /// Expected amount.
    pub expected: u64,
    /// Actual amount found.
    pub actual: u64,
    /// Difference (signed — positive = surplus, negative = shortfall).
    pub delta: i128,
    /// Who owns this discrepancy (responsible party).
    pub owner: String,
    /// Current status.
    pub status: DiscrepancyStatus,
    /// Resolution details, if resolved.
    pub resolution: Option<Resolution>,
    /// Timestamp when discovered (ms).
    pub discovered_at_ms: u64,
    /// Timestamp when resolved (ms), if resolved.
    pub resolved_at_ms: Option<u64>,
}

/// The 10 daily reconciliation checks per Spec §14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconciliationCheck {
    /// Provider payment records ↔ payment orders.
    ProviderToOrder,
    /// Payment orders ↔ OMNIA allocation events.
    OrderToAllocation,
    /// Treasury inventory (reserved vs. available).
    TreasuryInventory,
    /// Total minted vs. total burned.
    MintedVsBurned,
    /// Outstanding refund liability.
    OutstandingRefunds,
    /// Orders in MANUAL_REVIEW state.
    ManualReviewBacklog,
    /// Failed or uncertain on-chain transactions.
    FailedUncertainOnChain,
    /// Subsidy / provider-fee totals.
    SubsidyProviderFee,
    /// Merchant settlement records.
    MerchantSettlement,
    /// Incident/exception sign-off.
    IncidentSignOff,
}

/// Discrepancy status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscrepancyStatus {
    /// Discovered, not yet investigated.
    Open,
    /// Under investigation.
    Investigating,
    /// Root cause identified.
    RootCauseIdentified,
    /// Resolved.
    Resolved,
    /// Escalated to management.
    Escalated,
}

/// Resolution of a discrepancy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    /// What was done.
    pub action: String,
    /// Who approved the resolution.
    pub approved_by: String,
    /// Corrective ledger entry ID, if a correction was posted.
    pub correction_entry_id: Option<String>,
}

// --- Daily Reconciliation Report ---

/// A daily reconciliation report covering all 10 checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyReconciliationReport {
    /// The date (epoch days or ISO string).
    pub report_date: String,
    /// Timestamp when report was generated (ms).
    pub generated_at_ms: u64,
    /// Who generated the report.
    pub generated_by: String,
    /// Individual check results.
    pub checks: Vec<CheckResult>,
    /// Discrepancies found.
    pub discrepancies: Vec<Discrepancy>,
    /// Summary counts.
    pub summary: ReconciliationSummary,
    /// Whether all checks passed (no discrepancies).
    pub all_passed: bool,
}

/// Result of a single reconciliation check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Which check was performed.
    pub check_type: ReconciliationCheck,
    /// Whether the check passed.
    pub passed: bool,
    /// Details (e.g., expected vs. actual, record counts).
    pub details: String,
    /// Timestamp of the check (ms).
    pub checked_at_ms: u64,
}

/// Summary counts for a daily report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconciliationSummary {
    /// Total checks performed.
    pub total_checks: u64,
    /// Checks passed.
    pub passed: u64,
    /// Checks failed.
    pub failed: u64,
    /// Total discrepancies found.
    pub total_discrepancies: u64,
    /// Discrepancies still open.
    pub open_discrepancies: u64,
    /// Total discrepancy value (absolute, in smallest unit).
    pub total_discrepancy_value: u64,
}

impl DailyReconciliationReport {
    /// Create an empty report for the given date.
    pub fn new(report_date: String, generated_by: String, now_ms: u64) -> Self {
        Self {
            report_date,
            generated_at_ms: now_ms,
            generated_by,
            checks: Vec::new(),
            discrepancies: Vec::new(),
            summary: ReconciliationSummary::default(),
            all_passed: true,
        }
    }

    /// Add a check result.
    pub fn add_check(&mut self, result: CheckResult) {
        if !result.passed {
            self.all_passed = false;
        }
        self.summary.total_checks = self.summary.total_checks.saturating_add(1);
        if result.passed {
            self.summary.passed = self.summary.passed.saturating_add(1);
        } else {
            self.summary.failed = self.summary.failed.saturating_add(1);
        }
        self.checks.push(result);
    }

    /// Add a discrepancy.
    pub fn add_discrepancy(&mut self, discrepancy: Discrepancy) {
        self.all_passed = false;
        self.summary.total_discrepancies = self.summary.total_discrepancies.saturating_add(1);
        if matches!(
            discrepancy.status,
            DiscrepancyStatus::Open | DiscrepancyStatus::Investigating
        ) {
            self.summary.open_discrepancies = self.summary.open_discrepancies.saturating_add(1);
        }
        self.summary.total_discrepancy_value = self
            .summary
            .total_discrepancy_value
            .saturating_add(discrepancy.delta.unsigned_abs() as u64);
        self.discrepancies.push(discrepancy);
    }
}

// --- Order Reconciliation Status ---

/// Reconciliation status for a single payment order.
/// Tracks whether the order's financial flows have been fully reconciled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderReconciliation {
    /// Order ID.
    pub order_id: String,
    /// Whether the provider record matches the order.
    pub provider_reconciled: bool,
    /// Whether the OMNIA allocation matches the order.
    pub allocation_reconciled: bool,
    /// Whether the treasury inventory deduction matches.
    pub inventory_reconciled: bool,
    /// Whether the wallet credit matches the order.
    pub wallet_reconciled: bool,
    /// Whether the refund (if any) has been reconciled.
    pub refund_reconciled: bool,
    /// Overall reconciliation status.
    pub status: ReconciliationStatus,
}

impl OrderReconciliation {
    /// Create a new unreconciled order reconciliation.
    pub fn new(order_id: String) -> Self {
        Self {
            order_id,
            provider_reconciled: false,
            allocation_reconciled: false,
            inventory_reconciled: false,
            wallet_reconciled: false,
            refund_reconciled: false,
            status: ReconciliationStatus::Unreconciled,
        }
    }

    /// Update overall status based on individual flags.
    /// Takes the order state into account — terminal states have different requirements.
    pub fn update_status(&mut self, order_state: PaymentState) {
        let required_reconciliations = match order_state {
            PaymentState::Delivered => 4, // provider + allocation + inventory + wallet
            PaymentState::Refunded => 3,  // provider + refund + inventory (return)
            PaymentState::Cancelled => 1, // provider (or none if cancelled before payment)
            _ => 0,                       // in-flight orders aren't fully reconciled yet
        };

        let completed = self.provider_reconciled as u8
            + self.allocation_reconciled as u8
            + self.inventory_reconciled as u8
            + self.wallet_reconciled as u8
            + self.refund_reconciled as u8;

        self.status = if required_reconciliations == 0 {
            ReconciliationStatus::Unreconciled
        } else if completed as u8 >= required_reconciliations {
            ReconciliationStatus::Reconciled
        } else {
            ReconciliationStatus::PartiallyReconciled
        };
    }
}

/// Overall reconciliation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconciliationStatus {
    /// Not yet reconciled.
    Unreconciled,
    /// Some but not all reconciliation checks passed.
    PartiallyReconciled,
    /// All required reconciliation checks passed.
    Reconciled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_report_tracks_checks() {
        let mut report = DailyReconciliationReport::new("2026-08-15".into(), "auto".into(), 0);
        report.add_check(CheckResult {
            check_type: ReconciliationCheck::ProviderToOrder,
            passed: true,
            details: "all matched".into(),
            checked_at_ms: 1000,
        });
        report.add_check(CheckResult {
            check_type: ReconciliationCheck::TreasuryInventory,
            passed: false,
            details: "shortfall of 500 OMNIA".into(),
            checked_at_ms: 2000,
        });
        assert!(!report.all_passed);
        assert_eq!(report.summary.total_checks, 2);
        assert_eq!(report.summary.passed, 1);
        assert_eq!(report.summary.failed, 1);
    }

    #[test]
    fn discrepancy_tracking() {
        let mut report = DailyReconciliationReport::new("2026-08-15".into(), "auto".into(), 0);
        report.add_discrepancy(Discrepancy {
            discrepancy_id: "d-1".into(),
            check_type: ReconciliationCheck::TreasuryInventory,
            description: "inventory shortfall".into(),
            expected: 1000,
            actual: 500,
            delta: -500,
            owner: "treasury".into(),
            status: DiscrepancyStatus::Open,
            resolution: None,
            discovered_at_ms: 1000,
            resolved_at_ms: None,
        });
        assert_eq!(report.summary.total_discrepancies, 1);
        assert_eq!(report.summary.open_discrepancies, 1);
        assert_eq!(report.summary.total_discrepancy_value, 500);
    }

    #[test]
    fn order_reconciliation_delivered() {
        let mut rec = OrderReconciliation::new("order-1".into());
        assert_eq!(rec.status, ReconciliationStatus::Unreconciled);

        rec.provider_reconciled = true;
        rec.allocation_reconciled = true;
        rec.inventory_reconciled = true;
        rec.wallet_reconciled = true;
        rec.update_status(PaymentState::Delivered);
        assert_eq!(rec.status, ReconciliationStatus::Reconciled);
    }

    #[test]
    fn order_reconciliation_partial() {
        let mut rec = OrderReconciliation::new("order-1".into());
        rec.provider_reconciled = true;
        rec.update_status(PaymentState::Delivered);
        assert_eq!(rec.status, ReconciliationStatus::PartiallyReconciled);
    }

    #[test]
    fn order_reconciliation_refunded() {
        let mut rec = OrderReconciliation::new("order-1".into());
        rec.provider_reconciled = true;
        rec.refund_reconciled = true;
        rec.inventory_reconciled = true;
        rec.update_status(PaymentState::Refunded);
        assert_eq!(rec.status, ReconciliationStatus::Reconciled);
    }
}
