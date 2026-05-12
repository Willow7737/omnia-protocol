//! Identity shard operations
//!
//! Defines the lifecycle operations for DIDs: create, update, recover,
//! verify, and agent management.

use serde::{Deserialize, Serialize};

use super::state::DidDocument;

/// A DID string in the format `did:omnia:<hex_public_key>`.
pub type Did = String;

/// An update to apply to a DID document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DidUpdate {
    /// Add a new authentication method.
    AddAuthentication {
        /// The public key of the new authentication method.
        public_key: [u8; 32],
    },
    /// Remove an existing authentication method.
    RemoveAuthentication {
        /// The public key to remove.
        public_key: [u8; 32],
    },
    /// Add a service endpoint.
    AddService {
        /// Service identifier.
        service_id: String,
        /// Service endpoint URL.
        endpoint: String,
    },
}

/// A share from a guardian for social recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryShare {
    /// The guardian who provided this share.
    pub guardian: [u8; 32],
    /// The encrypted share data.
    pub share_data: Vec<u8>,
}

/// An AI agent identity linked to a DID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    /// Unique agent identifier.
    pub agent_id: [u8; 32],
    /// Human-readable name for the agent.
    pub name: String,
    /// The agent's public key.
    pub public_key: [u8; 32],
}

/// Operations supported by the Identity shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdentityOp {
    /// Create a new DID with the given document.
    CreateDid {
        /// The initial DID document.
        document: DidDocument,
    },
    /// Update an existing DID document.
    UpdateDid {
        /// The DID to update.
        did: Did,
        /// The updates to apply.
        updates: Vec<DidUpdate>,
    },
    /// Recover a DID using guardian shares.
    RecoverDid {
        /// The DID to recover.
        did: Did,
        /// Recovery shares from guardians.
        recovery_shares: Vec<RecoveryShare>,
    },
    /// Verify that a DID exists and is active.
    VerifyDid {
        /// The DID to verify.
        did: Did,
    },
    /// Add an AI agent identity to a DID.
    AddAgent {
        /// The DID that owns the agent.
        did: Did,
        /// The agent to add.
        agent: AgentIdentity,
    },
}
