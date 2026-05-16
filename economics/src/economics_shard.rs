//! Economics shard integration
//!
//! The `EconomicsShard` integrates the economics layer with the existing
//! Omnia shard infrastructure. It processes `EconomicsOp` operations and
//! delegates them to the appropriate subsystem (UBC, useful work, or
//! governance).
//!
//! The economics layer does not hold financial state directly — it uses
//! the financial shard's balance tracking with `UbcToken` semantics.
//! However, it does maintain its own `QuotaSystem` and `GovernanceState`
//! for managing UBC allowances and voting.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::EconomicsError;
use crate::fixed_point::DecayRate;
use crate::governance::{GovernanceState, VoteChoice};
use crate::quota::QuotaSystem;
use crate::useful_work::UsefulWorkProof;

/// Operations supported by the Economics layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EconomicsOp {
    /// Mint UBC to a DID (admin/epoch minting).
    MintUbc {
        /// DID to mint UBC for.
        did: String,
        /// Amount of UBC to mint.
        amount: u64,
    },
    /// Spend UBC from a DID's balance (transaction fee or compute cost).
    SpendUbc {
        /// DID to spend UBC from.
        did: String,
        /// Amount of UBC to spend.
        amount: u64,
    },
    /// Submit proof of useful work for a reward.
    SubmitWork {
        /// DID that performed the work.
        did: String,
        /// Proof of the useful work.
        proof: UsefulWorkProof,
    },
    /// Create a new governance proposal.
    CreateProposal {
        /// Unique proposal identifier.
        id: String,
        /// Human-readable description.
        description: String,
        /// Epoch when the proposal expires.
        expires_at_epoch: u64,
    },
    /// Cast a vote on a governance proposal.
    Vote {
        /// DID of the voter.
        did: String,
        /// ID of the proposal to vote on.
        proposal_id: String,
        /// The vote choice (For, Against, Abstain).
        choice: VoteChoice,
    },
    /// Register a new DID in the quota system.
    RegisterDid {
        /// DID to register.
        did: String,
    },
    /// Advance the epoch, resetting all UBC balances.
    AdvanceEpoch,
}

/// The full state of the Economics shard.
///
/// Combines the quota system (UBC token tracking), governance state
/// (quadratic voting with decay), and a registry of useful-work
/// proofs that have been submitted and verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicsState {
    /// The quota system managing UBC tokens for all DIDs.
    pub quota: QuotaSystem,
    /// The governance state for quadratic voting and proposals.
    pub governance: GovernanceState,
    /// Verified useful-work proofs keyed by result hash.
    pub verified_work: HashMap<[u8; 32], UsefulWorkProof>,
}

impl EconomicsState {
    /// Version byte prefixed to serialized snapshots for format migration.
    const ECONOMICS_STATE_VERSION: u8 = 1;

    /// Create a new economics state with default parameters.
    ///
    /// Uses the default UBC quota (1000 units/month) and a governance
    /// decay rate of 10% per inactive epoch.
    pub fn new() -> Self {
        Self {
            quota: QuotaSystem::default_system(),
            governance: GovernanceState::new(DecayRate::ten_percent()),
            verified_work: HashMap::new(),
        }
    }

    /// Create a new economics state with custom parameters.
    pub fn with_params(default_quota: u64, epoch_duration_ms: u64, decay_rate: DecayRate) -> Self {
        Self {
            quota: QuotaSystem::new(default_quota, epoch_duration_ms),
            governance: GovernanceState::new(decay_rate),
            verified_work: HashMap::new(),
        }
    }

    /// Apply an economics operation, mutating state.
    pub fn apply(&mut self, op: &EconomicsOp, current_epoch: u64) -> Result<(), EconomicsError> {
        match op {
            EconomicsOp::MintUbc { did, amount } => {
                if !self.quota.is_registered(did) {
                    self.quota.register_did(did);
                }
                self.quota.reward(did, *amount)
            }
            EconomicsOp::SpendUbc { did, amount } => self.quota.spend(did, *amount),
            EconomicsOp::SubmitWork { did, proof } => {
                proof.validate()?;
                self.verified_work.insert(proof.result_hash, proof.clone());
                let reward = proof.reward_amount();
                self.quota.reward(did, reward)
            }
            EconomicsOp::CreateProposal {
                id,
                description,
                expires_at_epoch,
            } => self.governance.create_proposal(
                id.clone(),
                description.clone(),
                *expires_at_epoch,
                current_epoch,
            ),
            EconomicsOp::Vote {
                did,
                proposal_id,
                choice,
            } => self
                .governance
                .vote(did, proposal_id, choice.clone(), current_epoch),
            EconomicsOp::RegisterDid { did } => {
                self.quota.register_did(did);
                Ok(())
            }
            EconomicsOp::AdvanceEpoch => {
                self.quota.advance_epoch();
                Ok(())
            }
        }
    }

    /// Query the UBC balance of a DID.
    pub fn balance_of(&self, did: &str) -> Option<u64> {
        self.quota.balance_of(did)
    }

    /// Get the current epoch.
    pub fn current_epoch(&self) -> u64 {
        self.quota.current_epoch
    }

    /// Serialize the economics state to bytes.
    ///
    /// The output is prefixed with a version byte to support future
    /// state-format migrations.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![Self::ECONOMICS_STATE_VERSION];
        bytes.extend(postcard::to_allocvec(self).expect("EconomicsState serialization cannot fail"));
        bytes
    }

    /// Deserialize economics state from bytes.
    ///
    /// Reads and validates the version byte before deserializing the
    /// payload. Returns an error if the version is unsupported.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        if bytes.is_empty() {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        let version = bytes[0];
        if version != Self::ECONOMICS_STATE_VERSION {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        postcard::from_bytes(&bytes[1..])
    }
}

impl Default for EconomicsState {
    fn default() -> Self {
        Self::new()
    }
}
