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

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::EconomicsError;

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
    pub fn new(id: String, description: String, created_at_epoch: u64, expires_at_epoch: u64) -> Self {
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
        self.votes_for.saturating_add(self.votes_against).saturating_add(self.votes_abstain)
    }
}

/// The full governance state, tracking voting weights, activity, and proposals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceState {
    /// DID → base voting weight (quadratic: weight = sqrt(stake)).
    pub voting_weights: HashMap<String, u64>,
    /// DID → last epoch when the DID participated in a vote.
    pub last_active: HashMap<String, u64>,
    /// Decay rate per inactive epoch (e.g., 0.1 = 10% decay per epoch).
    pub decay_rate: f64,
    /// Active proposals keyed by ID.
    pub proposals: HashMap<String, Proposal>,
}

impl GovernanceState {
    /// Create a new governance state with the specified decay rate.
    ///
    /// The decay rate is clamped to [0.0, 1.0] to prevent invalid
    /// exponential calculations.
    pub fn new(decay_rate: f64) -> Self {
        Self {
            voting_weights: HashMap::new(),
            last_active: HashMap::new(),
            decay_rate: decay_rate.clamp(0.0, 1.0),
            proposals: HashMap::new(),
        }
    }

    /// Set the voting weight for a DID based on their stake.
    ///
    /// The weight is calculated as `sqrt(stake)`, with a minimum
    /// weight of 1 for any non-zero stake. This quadratic formula
    /// means that doubling your influence requires quadrupling your
    /// stake, preventing plutocratic dominance.
    pub fn set_weight(&mut self, did: &str, stake: u64) {
        let weight = (stake as f64).sqrt() as u64;
        self.voting_weights.insert(did.to_string(), weight.max(1));
        self.last_active.insert(did.to_string(), 0);
    }

    /// Calculate the effective voting weight for a DID at the current epoch.
    ///
    /// The effective weight is the base weight multiplied by a decay
    /// factor that decreases exponentially with the number of inactive
    /// epochs: `weight * (1 - decay_rate)^inactive_epochs`.
    ///
    /// A DID that has never voted has an effective weight of zero
    /// (since `last_active` defaults to 0 and the number of inactive
    /// epochs will be the full current epoch).
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

        // Exponential decay: weight * (1 - decay_rate)^inactive_epochs
        let decay_factor = (1.0 - self.decay_rate).powi(inactive_epochs as i32);
        (base_weight as f64 * decay_factor) as u64
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
        bincode::serialize(self).expect("GovernanceState serialization cannot fail")
    }

    /// Deserialize governance state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}
