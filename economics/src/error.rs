//! Economics error types
//!
//! Defines all error variants for the economics layer: UBC operations,
//! useful-work verification, and governance actions.

use thiserror::Error;

/// Errors that can occur during economics operations.
#[derive(Debug, Error)]
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
}
