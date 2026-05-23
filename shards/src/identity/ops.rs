//! Identity shard operations
//!
//! Defines the lifecycle operations for DIDs: create, update, recover,
//! verify, agent management, biometric enrollment/verification, and
//! agent revocation. Extended in Layer 4 with Shamir's Secret Sharing
//! recovery, privacy-preserving biometric anchors, and AI agent
//! capability-based access control.

use serde::{Deserialize, Serialize};

use super::recovery::RecoveryShare;
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
    /// Recover a DID using Shamir's Secret Sharing shares.
    RecoverDid {
        /// The DID to recover.
        did: Did,
        /// Recovery shares (at least threshold required).
        shares: Vec<RecoveryShare>,
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
        agent: super::agent::AgentIdentity,
    },
    /// Enroll a biometric anchor for a DID.
    ///
    /// The raw template is NOT stored — only a salted commitment.
    EnrollBiometric {
        /// The DID to enroll the biometric for.
        did: Did,
        /// The raw biometric template (not persisted).
        template: Vec<u8>,
        /// Algorithm identifier (e.g., "fingerprint_v2").
        algorithm: String,
    },
    /// Verify a biometric against the stored commitment.
    VerifyBiometric {
        /// The DID to verify the biometric for.
        did: Did,
        /// The fresh biometric template to verify.
        template: Vec<u8>,
    },
    /// Revoke an AI agent, disabling all its capabilities.
    RevokeAgent {
        /// The DID of the agent to revoke.
        agent_did: Did,
    },
    /// Configure Shamir's Secret Sharing recovery for a DID.
    ConfigureRecovery {
        /// The DID to configure recovery for.
        did: Did,
        /// The secret to split into shares (e.g., the DID's private key).
        secret: Vec<u8>,
        /// Minimum number of shares required for recovery (K).
        threshold: u8,
        /// Total number of shares to create (N).
        total_shares: u8,
    },
}
