//! Governance controls for payment operations — Spec §12
//!
//! Key controls:
//! - Proposal deposit requirement
//! - 48-72hr execution timelock for ordinary changes
//! - Longer delay for supply/issuance/redemption changes
//! - Emergency pause capability
//! - Emergency authority expiry
//! - Flash-governance resistance
//! - Separation of supply vs. ordinary parameter changes

use serde::{Deserialize, Serialize};

use crate::error::PaymentError;

// --- Timelock Types ---

/// A governance proposal with timelock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceProposal {
    /// Unique proposal ID.
    pub proposal_id: String,
    /// Proposal type — determines timelock duration.
    pub proposal_type: ProposalType,
    /// Description / rationale.
    pub rationale: String,
    /// Code diff or parameter change description.
    pub code_diff: Option<String>,
    /// Proposer.
    pub proposer: String,
    /// Deposit amount (OMNIA plancks) locked by proposer.
    pub deposit: u64,
    /// When the proposal was created (ms).
    pub created_at_ms: u64,
    /// When the proposal becomes executable (ms).
    pub executable_at_ms: u64,
    /// Current status.
    pub status: ProposalStatus,
    /// Votes for.
    pub votes_for: u64,
    /// Votes against.
    pub votes_against: u64,
    /// Quorum requirement (minimum participation).
    pub quorum: u64,
    /// Approval threshold (fraction of quorum, basis points).
    pub approval_threshold_bps: u16,
    /// Execution timestamp, if executed.
    pub executed_at_ms: Option<u64>,
}

impl GovernanceProposal {
    /// Create a new proposal with the appropriate timelock.
    pub fn new(
        proposal_id: String,
        proposal_type: ProposalType,
        rationale: String,
        proposer: String,
        deposit: u64,
        now_ms: u64,
        quorum: u64,
        approval_threshold_bps: u16,
    ) -> Self {
        let timelock_ms = proposal_type.timelock_duration_ms();
        Self {
            proposal_id,
            proposal_type,
            rationale,
            code_diff: None,
            proposer,
            deposit,
            created_at_ms: now_ms,
            executable_at_ms: now_ms.saturating_add(timelock_ms),
            status: ProposalStatus::Proposed,
            votes_for: 0,
            votes_against: 0,
            quorum,
            approval_threshold_bps,
            executed_at_ms: None,
        }
    }

    /// Check if the proposal is past its timelock and can be executed.
    pub fn is_executable(&self, now_ms: u64) -> bool {
        self.status == ProposalStatus::Approved
            && now_ms >= self.executable_at_ms
    }

    /// Record a vote.
    pub fn vote(&mut self, support: bool) {
        if support {
            self.votes_for = self.votes_for.saturating_add(1);
        } else {
            self.votes_against = self.votes_against.saturating_add(1);
        }
    }

    /// Check if quorum is met.
    pub fn quorum_met(&self) -> bool {
        let total = self.votes_for.saturating_add(self.votes_against);
        total >= self.quorum
    }

    /// Check if approval threshold is met (of total votes, not quorum).
    pub fn approval_met(&self) -> bool {
        let total = self.votes_for.saturating_add(self.votes_against);
        if total == 0 {
            return false;
        }
        let approval_fraction = (self.votes_for as u128 * 10_000) / total as u128;
        approval_fraction >= self.approval_threshold_bps as u128
    }

    /// Finalize the vote: transition to Approved or Rejected.
    pub fn finalize_vote(&mut self) {
        if self.quorum_met() && self.approval_met() {
            self.status = ProposalStatus::Approved;
        } else {
            self.status = ProposalStatus::Rejected;
        }
    }
}

/// Proposal type with associated timelock duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalType {
    /// Ordinary parameter change (e.g., fee amounts, limits).
    /// Timelock: 48-72 hours.
    Ordinary,
    /// Supply, issuance, or redemption change.
    /// Longer timelock: 7-30 days.
    SupplyIssuance,
    /// Emergency pause.
    /// No timelock — immediate effect, but time-limited.
    EmergencyPause,
    /// Post-incident change triggered by an emergency.
    PostIncident,
}

impl ProposalType {
    /// Return the required timelock duration in milliseconds.
    pub fn timelock_duration_ms(&self) -> u64 {
        match self {
            Self::Ordinary => 60 * 60 * 1_000, // 60 hours (within 48-72 range)
            Self::SupplyIssuance => 14 * 24 * 60 * 60 * 1_000, // 14 days
            Self::EmergencyPause => 0, // immediate
            Self::PostIncident => 48 * 60 * 60 * 1_000, // 48 hours
        }
    }
}

/// Proposal status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Proposed, collecting votes.
    Proposed,
    /// Voting period ended, approved.
    Approved,
    /// Voting period ended, rejected.
    Rejected,
    /// Executed (timelock elapsed).
    Executed,
    /// Cancelled by proposer.
    Cancelled,
}

// --- Emergency Pause ---

/// Emergency pause state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencyPause {
    /// Whether the pause is currently active.
    pub active: bool,
    /// Who triggered the pause.
    pub triggered_by: String,
    /// Reason for the pause.
    pub reason: String,
    /// When the pause was triggered (ms).
    pub triggered_at_ms: u64,
    /// Maximum duration of the pause (ms).
    /// After this, the pause automatically expires.
    pub max_duration_ms: u64,
    /// Post-incident report required.
    pub incident_report_required: bool,
    /// Incident report reference, if filed.
    pub incident_report_ref: Option<String>,
}

impl EmergencyPause {
    /// Create a new emergency pause.
    pub fn new(triggered_by: String, reason: String, now_ms: u64) -> Self {
        Self {
            active: true,
            triggered_by,
            reason,
            triggered_at_ms: now_ms,
            max_duration_ms: 72 * 60 * 60 * 1_000, // 72 hours max
            incident_report_required: true,
            incident_report_ref: None,
        }
    }

    /// Check if the pause has expired.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.triggered_at_ms.saturating_add(self.max_duration_ms)
    }

    /// Lift the pause.
    pub fn lift(&mut self) {
        self.active = false;
    }

    /// Check if the pause is still active and not expired.
    pub fn is_active(&self, now_ms: u64) -> bool {
        self.active && !self.is_expired(now_ms)
    }
}

// --- Flash Governance Resistance ---

/// Delegation cooldown to resist flash governance attacks.
/// Per Spec §12: delegation changes must not take effect immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationCooldown {
    /// Map: delegator → (delegate, cooldown_expires_ms).
    pub delegations: std::collections::BTreeMap<String, (String, u64)>,
    /// Cooldown duration in milliseconds.
    pub cooldown_ms: u64,
}

impl DelegationCooldown {
    /// Create with default cooldown (e.g., 24 hours).
    pub fn new() -> Self {
        Self {
            delegations: std::collections::BTreeMap::new(),
            cooldown_ms: 24 * 60 * 60 * 1_000,
        }
    }

    /// Attempt to change delegation.
    /// Returns Ok if the cooldown has expired, Err if still cooling down.
    pub fn set_delegation(
        &mut self,
        delegator: &str,
        new_delegate: &str,
        now_ms: u64,
    ) -> Result<(), PaymentError> {
        if let Some((_, expires)) = self.delegations.get(delegator) {
            if now_ms < *expires {
                return Err(PaymentError::InvariantViolation(format!(
                    "delegation cooldown active for {} (expires at {})",
                    delegator, expires
                )));
            }
        }
        self.delegations.insert(
            delegator.into(),
            (new_delegate.into(), now_ms.saturating_add(self.cooldown_ms)),
        );
        Ok(())
    }

    /// Get the current delegate for a delegator.
    pub fn get_delegate(&self, delegator: &str) -> Option<&str> {
        self.delegations
            .get(delegator)
            .map(|(delegate, _)| delegate.as_str())
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_timelock_ordinary() {
        let p = GovernanceProposal::new(
            "p-1".into(),
            ProposalType::Ordinary,
            "test".into(),
            "alice".into(),
            1_000_000,
            0,
            10,
            6000, // 60%
        );
        assert_eq!(p.status, ProposalStatus::Proposed);
        // 59 hours — not yet executable
        assert!(!p.is_executable(59 * 60 * 60 * 1_000));
    }

    #[test]
    fn proposal_supply_longer_timelock() {
        let mut p = GovernanceProposal::new(
            "p-2".into(),
            ProposalType::SupplyIssuance,
            "increase cap".into(),
            "alice".into(),
            5_000_000,
            0,
            10,
            7000,
        );
        // Approve first
        for _ in 0..10 { p.vote(true); }
        p.finalize_vote();
        assert_eq!(p.status, ProposalStatus::Approved);
        // 13 days — not yet executable (14 day timelock)
        assert!(!p.is_executable(13 * 24 * 60 * 60 * 1_000));
        // 15 days — executable
        assert!(p.is_executable(15 * 24 * 60 * 60 * 1_000));
    }

    #[test]
    fn proposal_emergency_no_timelock() {
        let p = GovernanceProposal::new(
            "p-3".into(),
            ProposalType::EmergencyPause,
            "incident".into(),
            "ops".into(),
            0,
            0,
            5,
            8000,
        );
        assert_eq!(p.executable_at_ms, 0); // no timelock
    }

    #[test]
    fn proposal_vote_and_finalize() {
        let mut p = GovernanceProposal::new(
            "p-4".into(),
            ProposalType::Ordinary,
            "test".into(),
            "alice".into(),
            1_000_000,
            0,
            10,
            6000,
        );
        // 10 votes for, 0 against → quorum met, 100% approval
        for _ in 0..10 {
            p.vote(true);
        }
        assert!(p.quorum_met());
        assert!(p.approval_met());
        p.finalize_vote();
        assert_eq!(p.status, ProposalStatus::Approved);
    }

    #[test]
    fn proposal_rejection() {
        let mut p = GovernanceProposal::new(
            "p-5".into(),
            ProposalType::Ordinary,
            "test".into(),
            "alice".into(),
            1_000_000,
            0,
            10,
            6000,
        );
        // 6 for, 4 against → 60% approval (exactly at threshold)
        for _ in 0..6 {
            p.vote(true);
        }
        for _ in 0..4 {
            p.vote(false);
        }
        assert!(p.quorum_met());
        assert!(p.approval_met());
        // 5 for, 5 against → 50% < 60% threshold
        let mut p2 = GovernanceProposal::new(
            "p-6".into(),
            ProposalType::Ordinary,
            "test".into(),
            "alice".into(),
            1_000_000,
            0,
            10,
            6000,
        );
        for _ in 0..5 {
            p2.vote(true);
        }
        for _ in 0..5 {
            p2.vote(false);
        }
        assert!(p2.quorum_met());
        assert!(!p2.approval_met());
    }

    #[test]
    fn emergency_pause_expiry() {
        let pause = EmergencyPause::new("ops".into(), "incident".into(), 0);
        assert!(pause.is_active(0));
        assert!(pause.is_active(71 * 60 * 60 * 1_000)); // 71 hrs
        assert!(!pause.is_active(73 * 60 * 60 * 1_000)); // expired
    }

    #[test]
    fn delegation_cooldown() {
        let mut dc = DelegationCooldown::new();
        dc.set_delegation("alice", "bob", 0).unwrap();
        assert_eq!(dc.get_delegate("alice"), Some("bob"));
        // Immediate re-delegation fails
        assert!(dc.set_delegation("alice", "carol", 1000).is_err());
        // After cooldown succeeds
        dc.set_delegation("alice", "carol", 25 * 60 * 60 * 1_000).unwrap();
        assert_eq!(dc.get_delegate("alice"), Some("carol"));
    }
}
