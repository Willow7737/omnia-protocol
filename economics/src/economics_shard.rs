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

use std::collections::{HashMap, HashSet};

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
    /// Admin keys authorized to submit work during the pre-ZK phase.
    pub admin_keys: HashSet<[u8; 32]>,
    /// Verifier public key for work-proof verification.
    ///
    /// When `None`, verification falls back to an all-zeros key with a loud
    /// warning — this is INSECURE and MUST be overridden in production.
    pub verifier_pubkey: Option<[u8; 32]>,
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
            admin_keys: HashSet::new(),
            verifier_pubkey: None,
        }
    }

    /// Create a new economics state with custom parameters.
    pub fn with_params(default_quota: u64, epoch_duration_ms: u64, decay_rate: DecayRate) -> Self {
        Self {
            quota: QuotaSystem::new(default_quota, epoch_duration_ms),
            governance: GovernanceState::new(decay_rate),
            verified_work: HashMap::new(),
            admin_keys: HashSet::new(),
            verifier_pubkey: None,
        }
    }

    /// Check if a key is an admin key authorized to submit work.
    fn is_admin(&self, key: &[u8; 32]) -> bool {
        self.admin_keys.contains(key)
    }

    /// Add an admin key authorized to submit work during the pre-ZK phase.
    pub fn add_admin_key(&mut self, key: [u8; 32]) {
        self.admin_keys.insert(key);
    }

    /// Apply an economics operation, mutating state.
    ///
    /// # Arguments
    ///
    /// * `op` — The economics operation to apply
    /// * `current_epoch` — The current epoch number
    /// * `event_creator` — Optional public key of the entity that created this event,
    ///   used for admin gating of sensitive operations like `SubmitWork`
    pub fn apply(
        &mut self,
        op: &EconomicsOp,
        current_epoch: u64,
        event_creator: Option<&[u8; 32]>,
    ) -> Result<(), EconomicsError> {
        match op {
            EconomicsOp::MintUbc { did, amount } => {
                // SECURITY (C-6 fix): Admin authorization is ALWAYS required
                // for minting when admin_keys is configured. When admin_keys
                // is empty (the default), a loud warning is logged and minting
                // is allowed for backward compatibility with test environments.
                //
                // Production deployments MUST call add_admin_key() before
                // accepting events. The node startup code should configure
                // admin keys from OMNIA_AUTHORIZED_CALLERS.
                if !self.admin_keys.is_empty() {
                    let submitter = event_creator.ok_or_else(|| {
                        EconomicsError::ValidationFailed("MintUbc requires authenticated event creator".into())
                    })?;
                    if !self.is_admin(submitter) {
                        return Err(EconomicsError::Unauthorized(
                            "MintUbc requires admin authorization. Use SubmitWork with verified proof instead.".into(),
                        ));
                    }
                } else {
                    tracing::warn!(
                        "MintUbc accepted without admin verification — admin_keys is empty. \
                         Configure admin_keys for production to prevent unauthorized minting."
                    );
                }
                if !self.quota.is_registered(did) {
                    self.quota.register_did(did);
                }
                self.quota.reward(did, *amount)
            }
            EconomicsOp::SpendUbc { did, amount } => {
                // SECURITY (C-7 fix): Bind the caller to the DID being spent.
                // When event_creator is provided, verify it matches the op's DID.
                // When event_creator is None (testing only), log a warning.
                if let Some(creator_pubkey) = event_creator {
                    let caller_did = format!("did:omnia:{}", hex::encode(creator_pubkey));
                    if caller_did != *did {
                        return Err(EconomicsError::Unauthorized(
                            "SpendUbc: caller's DID does not match the DID being spent".into(),
                        ));
                    }
                } else {
                    tracing::warn!("SpendUbc accepted without event_creator — testing mode only");
                }
                self.quota.spend(did, *amount)
            }
            EconomicsOp::SubmitWork { did, proof } => {
                // Gate work submission: only authorized submitters can mint UBC
                // Until real ZK verification is implemented, require admin authorization
                // when admin_keys is configured.
                if self.admin_keys.is_empty() {
                    // When no admin keys configured, log a warning but allow (testing mode)
                    tracing::warn!(
                        "SubmitWork accepted without admin verification - configure admin_keys for production"
                    );
                } else {
                    // Admin verification required
                    let submitter = event_creator.ok_or_else(|| {
                        EconomicsError::ValidationFailed("SubmitWork requires authenticated event creator".into())
                    })?;
                    if !self.is_admin(submitter) {
                        return Err(EconomicsError::Unauthorized("Only admins can submit work".into()));
                    }
                }
                proof.validate()?;
                // SECURITY: Verify the work proof before accepting it.
                // The verifier public key should be a well-known network parameter.
                // TODO: Pass the actual verifier public key from network config
                // rather than using a zeroed placeholder. This is safe for now
                // because the `production` feature gate in `verify()` returns
                // false (rejecting all proofs) until real ZK verification is
                // implemented. In non-production mode, stub verification logs
                // a warning.
                // SECURITY: Use a configurable verifier public key. The all-zeros default
                // allows any forged proof to pass — this MUST be overridden in production.
                let verifier_pubkey = self.verifier_pubkey.unwrap_or_else(|| {
                    tracing::error!(
                        "No verifier public key configured — work proof verification is INSECURE. \
                         Set a proper verifier key before deploying to production."
                    );
                    [0u8; 32]
                });
                if !proof.verify(&verifier_pubkey) {
                    return Err(EconomicsError::WorkProofInvalid);
                }
                if self.verified_work.contains_key(&proof.result_hash) {
                    return Err(EconomicsError::InvalidOperation(
                        "Duplicate work proof — this result hash has already been submitted".into(),
                    ));
                }
                self.verified_work.insert(proof.result_hash, proof.clone());
                let reward = proof.reward_amount();
                self.quota.reward(did, reward)
            }
            EconomicsOp::CreateProposal {
                id,
                description,
                expires_at_epoch,
            } => self
                .governance
                .create_proposal(id.clone(), description.clone(), *expires_at_epoch, current_epoch),
            EconomicsOp::Vote {
                did,
                proposal_id,
                choice,
            } => {
                // SECURITY (C-8 fix): Bind the caller to the voting DID.
                // When event_creator is provided, verify it matches the op's DID.
                // When event_creator is None (testing only), log a warning.
                if let Some(creator_pubkey) = event_creator {
                    let caller_did = format!("did:omnia:{}", hex::encode(creator_pubkey));
                    if caller_did != *did {
                        return Err(EconomicsError::Unauthorized(
                            "Vote: caller's DID does not match the voting DID".into(),
                        ));
                    }
                } else {
                    tracing::warn!("Vote accepted without event_creator — testing mode only");
                }
                self.governance.vote(did, proposal_id, choice.clone(), current_epoch)
            }
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
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        let mut bytes = vec![Self::ECONOMICS_STATE_VERSION];
        bytes.extend(postcard::to_allocvec(self)?);
        Ok(bytes)
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
            // FIXME: postcard doesn't support custom error types.
            // The `DeserializeUnexpectedEnd` error is misleading for a
            // version mismatch — it implies truncated input rather than
            // an incompatible format. Consider wrapping in a custom
            // `EconomicsStateError` enum that can carry the version number
            // and distinguish between "wrong version" and "truncated data".
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
