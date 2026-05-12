//! Economics shard — UBC token, useful work, and governance operations
//!
//! Defines the `EconomicsOp` enum and `EconomicsVoteChoice` for the
//! economics domain shard. The full validation and state logic lives
//! in the `omnia-economics` crate; this module provides the types
//! needed for shard routing in `omnia-shards`.

use serde::{Deserialize, Serialize};

/// Vote choice for governance proposals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EconomicsVoteChoice {
    /// Vote in favor.
    For,
    /// Vote against.
    Against,
    /// Abstain from voting.
    Abstain,
}

/// Operations supported by the Economics shard.
///
/// Mirrors the `EconomicsOp` in the `omnia-economics` crate using
/// only types available in `omnia-shards` (no circular dependency).
/// The `UsefulWorkProof` is represented as opaque bytes here; the
/// economics crate deserializes it internally.
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
        /// Serialized useful-work proof (opaque to the shards crate).
        proof: Vec<u8>,
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
        choice: EconomicsVoteChoice,
    },
    /// Register a new DID in the quota system.
    RegisterDid {
        /// DID to register.
        did: String,
    },
    /// Advance the epoch, resetting all UBC balances.
    AdvanceEpoch,
}
