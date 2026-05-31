//! Economics error types
//!
//! Defines all error variants for the economics layer: UBC operations,
//! useful-work verification, and governance actions.

use thiserror::Error;

/// Errors that can occur during economics operations.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EconomicsError {
    /// Insufficient UBC balance for a spend operation.
    #[error("Insufficient UBC: have {have}, need {need}")]
    InsufficientUbc {
        /// Current balance.
        have: u64,
        /// Required amount.
        need: u64,
    },

    /// The DID is not registered in the quota system.
    #[error("DID not registered in quota system: {0}")]
    DidNotRegistered(String),

    /// The voter's effective weight has decayed to zero (inactive too long).
    #[error("Inactive voter with zero weight: {0}")]
    InactiveVoter(String),

    /// A proposal with the same ID already exists.
    #[error("Duplicate proposal: {0}")]
    DuplicateProposal(String),

    /// The requested proposal does not exist.
    #[error("Proposal not found: {0}")]
    ProposalNotFound(String),

    /// The proposal has expired and can no longer receive votes.
    #[error("Proposal expired: {0}")]
    ProposalExpired(String),

    /// Useful-work proof verification failed.
    #[error("Useful-work proof verification failed")]
    WorkProofInvalid,

    /// An invalid amount was specified (e.g., zero).
    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    /// Quorum was not met — too few voters participated.
    ///
    /// The total votes cast on the proposal were less than
    /// `quorum_percentage`% of the eligible voters.
    #[error("Quorum not met for proposal {proposal_id}: {votes_cast} votes cast, need {quorum_percentage}% of {eligible_voters} eligible voters")]
    QuorumNotMet {
        /// The proposal that failed quorum.
        proposal_id: String,
        /// Total quadratic weight of votes cast.
        votes_cast: u64,
        /// Number of eligible voters (non-zero weight).
        eligible_voters: u64,
        /// Required quorum percentage (0–100).
        quorum_percentage: u64,
    },

    /// The proposal has not yet expired and cannot be finalized.
    #[error("Proposal not yet expired: {0}")]
    ProposalNotExpired(String),

    /// The proposal was defeated — majority voted against.
    #[error("Proposal defeated: {0}")]
    ProposalDefeated(String),

    /// The DID has already voted on this proposal.
    #[error("Duplicate vote on proposal {0}")]
    DuplicateVote(String),

    /// A validation check failed (e.g., unauthorized operation).
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    /// Unauthorized operation (e.g., minting without admin key).
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Duplicate work proof submitted.
    #[error("Duplicate work proof: result hash already submitted")]
    DuplicateWorkProof,

    /// Balance overflow — operation would exceed maximum balance.
    #[error("Balance overflow: operation would exceed maximum balance")]
    BalanceOverflow,

    /// Invalid operation (generic catch-all for domain errors).
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}
