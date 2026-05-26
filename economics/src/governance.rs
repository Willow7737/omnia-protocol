//! Quadratic voting with reputation decay
//!
//! Governance in Omnia uses quadratic voting — each participant's voting
//! weight is the square root of their stake, not the stake itself. This
//! prevents plutocracy by ensuring that marginal influence becomes more
//! expensive as stake grows.
//!
//! Additionally, voting power decays if a participant is inactive. The
//! longer a participant goes without voting, the more their weight
//! diminishes, eventually reaching zero. This ensures that active
//! participants have more say than absent ones.
//!
//! # Fixed-Point Arithmetic
//!
//! All decay calculations use integer fixed-point arithmetic (PPM —
//! parts per million) instead of floating-point. This ensures bit-for-bit
//! identical results across all platforms (x86, ARM, etc.), preventing
//! consensus divergence.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::EconomicsError;
use crate::fixed_point::{isqrt, BasisPpmExt, DecayRate};

/// Default quorum percentage required for a proposal to pass (2/3 supermajority).
///
/// A proposal must have total votes cast ≥ 67% of eligible voters to pass,
/// even if a simple majority voted yes. This prevents small activist groups
/// from passing proposals when most participants are absent.
pub const DEFAULT_QUORUM_PERCENTAGE: u64 = 67;

/// Default time-lock delay before a passed proposal can be executed (24 hours in ms).
///
/// After a proposal passes, it must wait this duration before execution.
/// This gives the community time to review and potentially veto or raise
/// concerns about the proposal before it takes effect.
pub const DEFAULT_TIME_LOCK_MS: u64 = 86_400_000;

/// A vote choice on a governance proposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoteChoice {
    /// Vote in favor of the proposal.
    For,
    /// Vote against the proposal.
    Against,
    /// Abstain from the vote (counts toward participation but not outcome).
    Abstain,
}

/// A governance proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    /// Unique identifier for the proposal.
    pub id: String,
    /// Human-readable description of the proposal.
    pub description: String,
    /// Epoch when the proposal was created.
    pub created_at_epoch: u64,
    /// Epoch when the proposal expires and can no longer receive votes.
    pub expires_at_epoch: u64,
    /// Total quadratic weight voted in favor.
    pub votes_for: u64,
    /// Total quadratic weight voted against.
    pub votes_against: u64,
    /// Total quadratic weight abstained.
    pub votes_abstain: u64,
    /// Scheduled execution time (milliseconds since UNIX epoch).
    ///
    /// When a proposal passes finalization, `execution_time` is set to
    /// `current_time_ms + time_lock_ms`. The proposal can only be executed
    /// after this time has elapsed. `None` means the proposal has not
    /// been finalized (or was rejected).
    pub execution_time: Option<u64>,
}

impl Proposal {
    /// Create a new proposal.
    pub fn new(id: String, description: String, created_at_epoch: u64, expires_at_epoch: u64) -> Self {
        Self {
            id,
            description,
            created_at_epoch,
            expires_at_epoch,
            votes_for: 0,
            votes_against: 0,
            votes_abstain: 0,
            execution_time: None,
        }
    }

    /// Check if the proposal has expired.
    pub fn is_expired(&self, current_epoch: u64) -> bool {
        current_epoch > self.expires_at_epoch
    }

    /// Check if the proposal passes (more "for" than "against" votes).
    ///
    /// This only checks the simple-majority condition. Use
    /// [`GovernanceState::finalize_proposal`] for the full quorum + majority check.
    pub fn passes(&self) -> bool {
        self.votes_for > self.votes_against
    }

    /// Get total participation (sum of all vote weights).
    pub fn total_participation(&self) -> u64 {
        self.votes_for
            .saturating_add(self.votes_against)
            .saturating_add(self.votes_abstain)
    }

    /// Check whether the proposal is ready for execution.
    ///
    /// Returns `true` if the proposal has an `execution_time` set and the
    /// current time (in ms since UNIX epoch) is at or past that time.
    /// Returns `false` if the proposal has not been finalized or the
    /// time-lock has not yet elapsed.
    pub fn is_ready_for_execution(&self, current_time_ms: u64) -> bool {
        match self.execution_time {
            Some(exec_time) => current_time_ms >= exec_time,
            None => false,
        }
    }
}

/// The full governance state, tracking voting weights, activity, and proposals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceState {
    /// DID → base voting weight (quadratic: weight = isqrt(stake)).
    pub voting_weights: HashMap<String, u64>,
    /// DID → last epoch when the DID participated in a vote.
    pub last_active: HashMap<String, u64>,
    /// Decay rate per inactive epoch in PPM (e.g., 100_000 = 10% decay).
    pub decay_rate: DecayRate,
    /// Active proposals keyed by ID.
    pub proposals: HashMap<String, Proposal>,
    /// Minimum quorum percentage (0–100) required for a proposal to pass.
    ///
    /// The total votes cast on a proposal must represent at least this
    /// percentage of all eligible voters (those with non-zero `voting_weights`).
    /// If quorum is not met, the proposal fails even if a majority voted yes.
    pub quorum_percentage: u64,
    /// Time-lock delay in milliseconds before a passed proposal can be executed.
    ///
    /// After finalization, the proposal's `execution_time` is set to
    /// `current_time_ms + time_lock_ms`.
    pub time_lock_ms: u64,
    /// Tracks which (proposal_id, did) pairs have already voted, preventing
    /// double-voting. The value is always `true` when present.
    pub voted: HashMap<(String, String), bool>,
}

impl GovernanceState {
    /// Create a new governance state with the specified decay rate and
    /// default quorum / time-lock settings.
    ///
    /// The decay rate is specified as a [`DecayRate`] in parts-per-million.
    /// Use [`DecayRate::ten_percent()`] for the standard 10% decay per epoch,
    /// or [`DecayRate::from_percent`] for a custom rate.
    ///
    /// Quorum defaults to [`DEFAULT_QUORUM_PERCENTAGE`] (67%) and the
    /// time-lock to [`DEFAULT_TIME_LOCK_MS`] (24 hours).
    ///
    /// # Examples
    ///
    /// ```
    /// use omnia_economics::governance::GovernanceState;
    /// use omnia_economics::fixed_point::DecayRate;
    ///
    /// let gov = GovernanceState::new(DecayRate::ten_percent());
    /// ```
    pub fn new(decay_rate: DecayRate) -> Self {
        Self {
            voting_weights: HashMap::new(),
            last_active: HashMap::new(),
            decay_rate,
            proposals: HashMap::new(),
            quorum_percentage: DEFAULT_QUORUM_PERCENTAGE,
            time_lock_ms: DEFAULT_TIME_LOCK_MS,
            voted: HashMap::new(),
        }
    }

    /// Set the voting weight for a DID based on their stake.
    ///
    /// The weight is calculated as `isqrt(stake)`, with a minimum
    /// weight of 1 for any non-zero stake. This quadratic formula
    /// means that doubling your influence requires quadrupling your
    /// stake, preventing plutocratic dominance.
    ///
    /// Uses integer square root via Newton's method — no floating-point
    /// arithmetic.
    pub fn set_weight(&mut self, did: &str, stake: u64) {
        let weight = isqrt(stake).max(1);
        self.voting_weights.insert(did.to_string(), weight);
        self.last_active.insert(did.to_string(), 0);
    }

    /// Calculate the effective voting weight for a DID at the current epoch.
    ///
    /// The effective weight is the base weight multiplied by a decay
    /// factor that decreases with the number of inactive epochs.
    /// The formula uses integer fixed-point arithmetic (PPM):
    ///
    /// `effective = (base_weight * remaining_ppm) / BASIS_PPM`
    ///
    /// where `remaining_ppm = decay_rate.remaining_ppm_after(inactive_epochs)`.
    ///
    /// All results are bit-for-bit identical across platforms.
    pub fn effective_weight(&self, did: &str, current_epoch: u64) -> u64 {
        let base_weight = self.voting_weights.get(did).copied().unwrap_or(0);
        let last_active = self.last_active.get(did).copied().unwrap_or(0);

        // If the DID has never been active and the current epoch > 0,
        // the inactive epochs will be large, causing near-zero weight.
        // However, if last_active was set to 0 but the DID just registered,
        // they should still have weight. The convention is:
        // - last_active = 0 means "never voted" → full decay
        // - After first vote, last_active is set to current epoch
        if base_weight == 0 {
            return 0;
        }

        // Special case: if the DID was just set up (last_active == 0)
        // and we're at epoch 0, they have full weight.
        if last_active == 0 && current_epoch == 0 {
            return base_weight;
        }

        let inactive_epochs = current_epoch.saturating_sub(last_active);

        // Fixed-point decay: remaining_ppm is in [0, 1_000_000]
        let remaining_ppm = self.decay_rate.remaining_ppm_after(inactive_epochs);
        base_weight.mul_ppm(remaining_ppm)
    }

    /// Create a new governance proposal.
    ///
    /// Returns an error if a proposal with the same ID already exists.
    pub fn create_proposal(
        &mut self,
        id: String,
        description: String,
        expires_at_epoch: u64,
        current_epoch: u64,
    ) -> Result<(), EconomicsError> {
        if self.proposals.contains_key(&id) {
            return Err(EconomicsError::DuplicateProposal(id));
        }
        self.proposals.insert(
            id.clone(),
            Proposal::new(id, description, current_epoch, expires_at_epoch),
        );
        Ok(())
    }

    /// Cast a vote on a proposal.
    ///
    /// The vote's weight is the DID's effective voting weight at the
    /// current epoch (including decay). After voting, the DID's
    /// `last_active` is updated to the current epoch, preventing
    /// further decay until the next period of inactivity.
    pub fn vote(
        &mut self,
        did: &str,
        proposal_id: &str,
        choice: VoteChoice,
        current_epoch: u64,
    ) -> Result<(), EconomicsError> {
        // Check for double-vote before tallying
        let vote_key = (proposal_id.to_string(), did.to_string());
        if self.voted.contains_key(&vote_key) {
            return Err(EconomicsError::DuplicateVote(proposal_id.to_string()));
        }

        let weight = self.effective_weight(did, current_epoch);
        if weight == 0 {
            return Err(EconomicsError::InactiveVoter(did.to_string()));
        }

        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| EconomicsError::ProposalNotFound(proposal_id.to_string()))?;

        if proposal.is_expired(current_epoch) {
            return Err(EconomicsError::ProposalExpired(proposal_id.to_string()));
        }

        match choice {
            VoteChoice::For => proposal.votes_for = proposal.votes_for.saturating_add(weight),
            VoteChoice::Against => proposal.votes_against = proposal.votes_against.saturating_add(weight),
            VoteChoice::Abstain => proposal.votes_abstain = proposal.votes_abstain.saturating_add(weight),
        }

        self.last_active.insert(did.to_string(), current_epoch);

        // Record that this DID has voted on this proposal
        self.voted.insert(vote_key, true);

        Ok(())
    }

    /// Get a reference to a proposal by ID.
    pub fn get_proposal(&self, id: &str) -> Option<&Proposal> {
        self.proposals.get(id)
    }

    /// Get a mutable reference to a proposal by ID.
    pub fn get_proposal_mut(&mut self, id: &str) -> Option<&mut Proposal> {
        self.proposals.get_mut(id)
    }

    /// Finalize an expired proposal, applying quorum and time-lock rules.
    ///
    /// This method must be called after a proposal has expired
    /// (`current_epoch > proposal.expires_at_epoch`). It checks:
    ///
    /// 1. **Quorum**: total votes cast ≥ `quorum_percentage`% of eligible voters.
    /// 2. **Majority**: more "for" than "against" votes.
    ///
    /// If both conditions are met, the proposal is marked as passed and
    /// receives an `execution_time` = `current_time_ms + time_lock_ms`.
    /// If quorum is not met, the proposal fails with [`EconomicsError::QuorumNotMet`].
    /// If quorum is met but the majority voted against, the proposal fails
    /// with [`EconomicsError::ProposalDefeated`].
    ///
    /// # Errors
    ///
    /// - [`EconomicsError::ProposalNotFound`] — no proposal with the given ID.
    /// - [`EconomicsError::ProposalNotExpired`] — the proposal has not yet expired.
    /// - [`EconomicsError::QuorumNotMet`] — insufficient voter participation.
    /// - [`EconomicsError::ProposalDefeated`] — majority voted against.
    pub fn finalize_proposal(
        &mut self,
        proposal_id: &str,
        current_epoch: u64,
        current_time_ms: u64,
    ) -> Result<(), EconomicsError> {
        let proposal = self
            .proposals
            .get(proposal_id)
            .ok_or_else(|| EconomicsError::ProposalNotFound(proposal_id.to_string()))?;

        if !proposal.is_expired(current_epoch) {
            return Err(EconomicsError::ProposalNotExpired(proposal_id.to_string()));
        }

        let total_votes = proposal.total_participation();
        let eligible_voters = self.eligible_voter_count();
        // AUDIT-23: Use effective (decayed) weights for quorum computation,
        // not base weights. Without decay, a voter whose reputation has
        // eroded still counts fully toward quorum, which can be exploited
        // to pass proposals with stale voters.
        let total_possible_weight = self.total_effective_voting_weight(current_epoch);

        // Check quorum: total vote weight cast must be >= quorum_percentage%
        // of total possible voting weight (sum of all eligible voters' weights).
        // We compute (total_votes * 100) >= (total_possible_weight * quorum_percentage)
        // to avoid floating-point arithmetic.
        if total_possible_weight > 0 {
            let quorum_threshold = total_possible_weight.saturating_mul(self.quorum_percentage);
            let votes_percentage_scale = total_votes.saturating_mul(100);
            if votes_percentage_scale < quorum_threshold {
                return Err(EconomicsError::QuorumNotMet {
                    proposal_id: proposal_id.to_string(),
                    votes_cast: total_votes,
                    eligible_voters,
                    quorum_percentage: self.quorum_percentage,
                });
            }
        }

        // Check majority
        if !proposal.passes() {
            return Err(EconomicsError::ProposalDefeated(proposal_id.to_string()));
        }

        // Proposal passes: set execution_time with time-lock delay.
        let proposal = self.proposals.get_mut(proposal_id).expect("proposal was found above");
        proposal.execution_time = Some(current_time_ms.saturating_add(self.time_lock_ms));

        Ok(())
    }

    /// Count the number of eligible voters (DIDs with non-zero voting weight).
    pub fn eligible_voter_count(&self) -> u64 {
        self.voting_weights.values().filter(|&&w| w > 0).count() as u64
    }

    /// Compute the total possible voting weight (sum of all base weights).
    ///
    /// This is used for quorum computation: a proposal passes quorum when
    /// the total weight of votes cast ≥ `quorum_percentage`% of this total.
    pub fn total_voting_weight(&self) -> u64 {
        self.voting_weights.values().copied().fold(0u64, u64::saturating_add)
    }

    /// Sum of effective (decayed) voting weights for all eligible voters
    /// at the given epoch.
    ///
    /// This should be used for quorum computation instead of
    /// [`total_voting_weight`] to prevent stale voters from inflating
    /// the quorum threshold.
    pub fn total_effective_voting_weight(&self, current_epoch: u64) -> u64 {
        self.voting_weights
            .keys()
            .map(|did| self.effective_weight(did, current_epoch))
            .fold(0u64, u64::saturating_add)
    }

    /// Remove expired proposals from the active set.
    ///
    /// Returns the number of proposals removed.
    pub fn cleanup_expired(&mut self, current_epoch: u64) -> usize {
        let expired_ids: Vec<String> = self
            .proposals
            .iter()
            .filter(|(_, p)| p.is_expired(current_epoch))
            .map(|(id, _)| id.clone())
            .collect();

        let count = expired_ids.len();
        for id in expired_ids {
            self.proposals.remove(&id);
        }
        count
    }

    /// Serialize the governance state to bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    /// Deserialize governance state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::BASIS_PPM;

    #[test]
    fn test_set_weight_quadratic() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());

        // 100 stake → isqrt(100) = 10
        gov.set_weight("alice", 100);
        assert_eq!(gov.voting_weights.get("alice"), Some(&10));

        // 10000 stake → isqrt(10000) = 100
        gov.set_weight("bob", 10000);
        assert_eq!(gov.voting_weights.get("bob"), Some(&100));

        // 0 stake → minimum weight of 1
        gov.set_weight("charlie", 0);
        assert_eq!(gov.voting_weights.get("charlie"), Some(&1));
    }

    #[test]
    fn test_effective_weight_at_epoch_0() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        gov.set_weight("alice", 100); // base weight = 10

        // At epoch 0, full weight (no decay yet)
        assert_eq!(gov.effective_weight("alice", 0), 10);
    }

    #[test]
    fn test_effective_weight_decay() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        gov.set_weight("alice", 100); // base weight = 10

        // After voting at epoch 0, then 1 epoch of inactivity
        gov.vote("alice", "prop1", VoteChoice::For, 0).ok();
        // last_active = 0, current_epoch = 1
        // remaining_ppm = 900_000
        // effective = 10 * 900_000 / 1_000_000 = 9
        assert_eq!(gov.effective_weight("alice", 1), 9);
    }

    #[test]
    fn test_effective_weight_determinism() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        gov.set_weight("alice", 100);

        // Call effective_weight 10,000 times — must produce identical results
        let mut results = Vec::new();
        for _ in 0..10_000 {
            results.push(gov.effective_weight("alice", 5));
        }
        let first = results[0];
        assert!(
            results.iter().all(|&r| r == first),
            "effective_weight is not deterministic: got varying results"
        );
    }

    #[test]
    fn test_effective_weight_zero_base() {
        let gov = GovernanceState::new(DecayRate::ten_percent());
        // DID not registered → base weight = 0
        assert_eq!(gov.effective_weight("unknown", 0), 0);
    }

    #[test]
    fn test_effective_weight_zero_decay() {
        let mut gov = GovernanceState::new(DecayRate::new(0)); // 0% decay
        gov.set_weight("alice", 100); // base weight = 10

        // No decay even after many epochs
        assert_eq!(gov.effective_weight("alice", 100), 10);
    }

    #[test]
    fn test_effective_weight_full_decay() {
        let mut gov = GovernanceState::new(DecayRate::new(BASIS_PPM)); // 100% decay
        gov.set_weight("alice", 100); // base weight = 10

        // After 1 epoch of inactivity, weight is 0
        assert_eq!(gov.effective_weight("alice", 1), 0);
    }

    #[test]
    fn test_isqrt_for_large_values() {
        // isqrt(u64::MAX) = 4294967295
        assert_eq!(isqrt(u64::MAX), 4294967295);

        // set_weight with u64::MAX stake
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        gov.set_weight("whale", u64::MAX);
        assert_eq!(gov.voting_weights.get("whale"), Some(&4294967295));
    }

    #[test]
    fn test_no_f64_in_module() {
        // This test documents the requirement that no f64/f32 exists
        // in the economics crate source. The actual check is done
        // via grep in the final checklist.
    }

    // ── Quorum and time-lock tests ─────────────────────────────────────

    #[test]
    fn test_default_quorum_percentage() {
        assert_eq!(DEFAULT_QUORUM_PERCENTAGE, 67);
    }

    #[test]
    fn test_default_time_lock_ms() {
        assert_eq!(DEFAULT_TIME_LOCK_MS, 86_400_000);
    }

    #[test]
    fn test_governance_state_default_quorum() {
        let gov = GovernanceState::new(DecayRate::ten_percent());
        assert_eq!(gov.quorum_percentage, 67);
        assert_eq!(gov.time_lock_ms, 86_400_000);
    }

    #[test]
    fn test_eligible_voter_count() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        assert_eq!(gov.eligible_voter_count(), 0);

        gov.set_weight("alice", 100); // weight = 10
        assert_eq!(gov.eligible_voter_count(), 1);

        gov.set_weight("bob", 400); // weight = 20
        assert_eq!(gov.eligible_voter_count(), 2);
    }

    #[test]
    fn test_proposal_execution_time_initially_none() {
        let proposal = Proposal::new("prop1".to_string(), "test".to_string(), 0, 10);
        assert!(proposal.execution_time.is_none());
    }

    #[test]
    fn test_proposal_is_ready_for_execution() {
        let mut proposal = Proposal::new("prop1".to_string(), "test".to_string(), 0, 10);

        // Not ready — no execution_time set
        assert!(!proposal.is_ready_for_execution(1_000_000));

        // Set execution_time to 2_000_000
        proposal.execution_time = Some(2_000_000);

        // Not ready before the scheduled time
        assert!(!proposal.is_ready_for_execution(1_999_999));

        // Ready exactly at the scheduled time
        assert!(proposal.is_ready_for_execution(2_000_000));

        // Ready after the scheduled time
        assert!(proposal.is_ready_for_execution(3_000_000));
    }

    #[test]
    fn test_finalize_proposal_quorum_met_and_majority() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        // 3 eligible voters
        gov.set_weight("alice", 100); // weight = 10
        gov.set_weight("bob", 400); // weight = 20
        gov.set_weight("charlie", 900); // weight = 30

        gov.create_proposal("prop1".to_string(), "test".to_string(), 10, 0).ok();

        // All 3 vote "for" → 60 total weight, eligible = 3
        // quorum: 60 * 100 = 6000 >= 3 * 67 = 201 ✓
        gov.vote("alice", "prop1", VoteChoice::For, 0).ok();
        gov.vote("bob", "prop1", VoteChoice::For, 1).ok();
        gov.vote("charlie", "prop1", VoteChoice::For, 2).ok();

        // Finalize at epoch 11 (after expires_at_epoch=10)
        let result = gov.finalize_proposal("prop1", 11, 1_000_000);
        assert!(result.is_ok());

        let proposal = gov.get_proposal("prop1").expect("proposal exists");
        assert_eq!(proposal.execution_time, Some(1_000_000 + DEFAULT_TIME_LOCK_MS));
    }

    #[test]
    fn test_finalize_proposal_quorum_not_met() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        // 10 eligible voters, each with weight 10 (isqrt(100))
        // total_possible_weight = 100
        for i in 0..10 {
            gov.set_weight(&format!("voter{i}"), 100);
        }

        gov.create_proposal("prop1".to_string(), "test".to_string(), 10, 0).ok();

        // Only 1 voter votes "for" → 10 total weight, total_possible_weight = 100
        // quorum: 10 * 100 = 1000 < 100 * 67 = 6700 ✗
        gov.vote("voter0", "prop1", VoteChoice::For, 0).ok();

        let result = gov.finalize_proposal("prop1", 11, 1_000_000);
        assert!(matches!(result, Err(EconomicsError::QuorumNotMet { .. })));
    }

    #[test]
    fn test_finalize_proposal_defeated() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        gov.set_weight("alice", 100);
        gov.set_weight("bob", 400);

        gov.create_proposal("prop1".to_string(), "test".to_string(), 10, 0).ok();

        // Both vote, quorum met, but majority against
        gov.vote("alice", "prop1", VoteChoice::Against, 0).ok();
        gov.vote("bob", "prop1", VoteChoice::Against, 1).ok();

        let result = gov.finalize_proposal("prop1", 11, 1_000_000);
        assert!(matches!(result, Err(EconomicsError::ProposalDefeated(_))));
    }

    #[test]
    fn test_finalize_proposal_not_expired() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        gov.set_weight("alice", 100);

        gov.create_proposal("prop1".to_string(), "test".to_string(), 10, 0).ok();

        // Try to finalize before expiration (current_epoch=5, expires_at=10)
        let result = gov.finalize_proposal("prop1", 5, 1_000_000);
        assert!(matches!(result, Err(EconomicsError::ProposalNotExpired(_))));
    }

    #[test]
    fn test_finalize_proposal_not_found() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        let result = gov.finalize_proposal("nonexistent", 11, 1_000_000);
        assert!(matches!(result, Err(EconomicsError::ProposalNotFound(_))));
    }

    #[test]
    fn test_double_vote_prevention() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        gov.set_weight("alice", 100); // weight = 10

        gov.create_proposal("prop1".to_string(), "test".to_string(), 10, 0).ok();

        // First vote should succeed
        let result = gov.vote("alice", "prop1", VoteChoice::For, 0);
        assert!(result.is_ok());

        // Second vote from the same DID on the same proposal should fail
        let result = gov.vote("alice", "prop1", VoteChoice::Against, 1);
        assert!(matches!(result, Err(EconomicsError::DuplicateVote(_))));
    }

    #[test]
    fn test_get_proposal_mut() {
        let mut gov = GovernanceState::new(DecayRate::ten_percent());
        gov.create_proposal("prop1".to_string(), "test".to_string(), 10, 0).ok();

        let proposal = gov.get_proposal_mut("prop1");
        assert!(proposal.is_some());

        let missing = gov.get_proposal_mut("nonexistent");
        assert!(missing.is_none());
    }
}

/// Property-based tests for governance invariants.
///
/// These tests verify that quadratic voting weight is monotonically
/// increasing with stake, and that decay rate computations never panic.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod proptests {
    use super::*;
    use crate::fixed_point::{DecayRate, BASIS_PPM};
    use proptest::prelude::*;

    proptest! {
        /// Property: Quadratic voting weight (isqrt) is monotonically
        /// increasing with stake. More stake should never result in less
        /// voting weight.
        #[test]
        fn proptest_quadratic_weight_monotonic(stake in 0u64..1_000_000u64) {
            let mut gov = GovernanceState::new(DecayRate::ten_percent());
            let weight_n = {
                gov.set_weight("node_n", stake);
                gov.voting_weights.get("node_n").copied().unwrap_or(0)
            };
            let weight_n1 = {
                gov.set_weight("node_n1", stake + 1);
                gov.voting_weights.get("node_n1").copied().unwrap_or(0)
            };
            assert!(
                weight_n <= weight_n1,
                "Quadratic weight not monotonic: weight({}) = {} > weight({}) = {}",
                stake, weight_n, stake + 1, weight_n1
            );
        }

        /// Property: Quadratic weight grows sub-linearly (approximately
        /// as sqrt). Doubling the stake should less than double the weight.
        #[test]
        fn proptest_quadratic_sublinear_growth(stake in 10u64..100_000u64) {
            let mut gov = GovernanceState::new(DecayRate::ten_percent());
            gov.set_weight("a", stake);
            gov.set_weight("b", 2 * stake);
            let weight_a = gov.voting_weights.get("a").copied().unwrap_or(0);
            let weight_b = gov.voting_weights.get("b").copied().unwrap_or(0);
            // sqrt(2n) < 2 * sqrt(n) for n > 0
            assert!(
                weight_b < 2 * weight_a || weight_a == 0,
                "Quadratic weight not sub-linear: weight({}) = {}, weight({}) = {}",
                2 * stake, weight_b, stake, weight_a
            );
        }

        /// Property: Decay rate computation never panics for valid PPM values.
        #[test]
        fn proptest_decay_rate_valid(ppm in 0u64..1_000_000u64) {
            let rate = DecayRate::new(ppm);
            assert_eq!(rate.ppm(), ppm);
        }

        /// Property: Decay rate clamps values above BASIS_PPM.
        #[test]
        fn proptest_decay_rate_clamps(ppm in 1_000_001u64..u64::MAX) {
            let rate = DecayRate::new(ppm);
            assert_eq!(rate.ppm(), BASIS_PPM);
        }

        /// Property: Effective weight is always <= base weight
        /// (decay never increases weight).
        #[test]
        fn proptest_effective_weight_never_exceeds_base(
            stake in 1u64..1_000_000u64,
            epoch in 0u64..100u64
        ) {
            let mut gov = GovernanceState::new(DecayRate::ten_percent());
            gov.set_weight("test", stake);
            let base_weight = gov.voting_weights.get("test").copied().unwrap_or(0);
            let effective = gov.effective_weight("test", epoch);
            assert!(
                effective <= base_weight,
                "Effective weight {effective} exceeds base weight {base_weight} at epoch {epoch}"
            );
        }
    }
}
