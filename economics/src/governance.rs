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
}

impl Proposal {
    /// Create a new proposal.
    pub fn new(
        id: String,
        description: String,
        created_at_epoch: u64,
        expires_at_epoch: u64,
    ) -> Self {
        Self {
            id,
            description,
            created_at_epoch,
            expires_at_epoch,
            votes_for: 0,
            votes_against: 0,
            votes_abstain: 0,
        }
    }

    /// Check if the proposal has expired.
    pub fn is_expired(&self, current_epoch: u64) -> bool {
        current_epoch > self.expires_at_epoch
    }

    /// Check if the proposal passes (more "for" than "against" votes).
    pub fn passes(&self) -> bool {
        self.votes_for > self.votes_against
    }

    /// Get total participation (sum of all vote weights).
    pub fn total_participation(&self) -> u64 {
        self.votes_for
            .saturating_add(self.votes_against)
            .saturating_add(self.votes_abstain)
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
}

impl GovernanceState {
    /// Create a new governance state with the specified decay rate.
    ///
    /// The decay rate is specified as a [`DecayRate`] in parts-per-million.
    /// Use [`DecayRate::ten_percent()`] for the standard 10% decay per epoch,
    /// or [`DecayRate::from_percent`] for a custom rate.
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
            VoteChoice::For => proposal.votes_for += weight,
            VoteChoice::Against => proposal.votes_against += weight,
            VoteChoice::Abstain => proposal.votes_abstain += weight,
        }

        self.last_active.insert(did.to_string(), current_epoch);
        Ok(())
    }

    /// Get a reference to a proposal by ID.
    pub fn get_proposal(&self, id: &str) -> Option<&Proposal> {
        self.proposals.get(id)
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
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("GovernanceState serialization cannot fail")
    }

    /// Deserialize governance state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

#[cfg(test)]
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
}

/// Property-based tests for governance invariants.
///
/// These tests verify that quadratic voting weight is monotonically
/// increasing with stake, and that decay rate computations never panic.
#[cfg(test)]
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
                "Effective weight {} exceeds base weight {} at epoch {}",
                effective, base_weight, epoch
            );
        }
    }
}
