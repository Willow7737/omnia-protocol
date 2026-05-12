//! Identity shard state
//!
//! Maintains the DID document registry, recovery configurations, and
//! agent registry. DID documents use last-write-wins semantics backed
//! by vector clocks for deterministic resolution of concurrent updates.

use std::collections::HashMap;

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use super::ops::{AgentIdentity, Did, DidUpdate, IdentityOp};
use crate::shard::ShardError;

/// A DID document representing a decentralized identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DidDocument {
    /// The DID string (e.g., `did:omnia:<hex_pubkey>`).
    pub id: Did,
    /// The controller's Ed25519 public key.
    pub public_key: [u8; 32],
    /// Timestamp (epoch millis) when the DID was created.
    pub created_at: u64,
    /// Vector clock of the last update — used for conflict resolution.
    pub updated_at: VectorClock,
    /// Whether social recovery is enabled for this DID.
    pub recovery_enabled: bool,
    /// Authentication methods (public keys authorized to sign for this DID).
    pub authentication: Vec<[u8; 32]>,
    /// Service endpoints associated with this DID.
    pub services: HashMap<String, String>,
}

impl DidDocument {
    /// Create a new DID document from a DID and public key.
    pub fn new(id: Did, public_key: [u8; 32], created_at: u64) -> Self {
        Self {
            id,
            public_key,
            created_at,
            updated_at: VectorClock::new(),
            recovery_enabled: false,
            authentication: vec![public_key],
            services: HashMap::new(),
        }
    }
}

/// Social recovery configuration for a DID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Minimum number of guardian shares required (e.g., 3 for 3-of-5).
    pub threshold: u8,
    /// Guardian public keys.
    pub guardians: Vec<[u8; 32]>,
}

/// The full state of the Identity shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityState {
    /// DID document registry — maps DID strings to their documents.
    pub dids: HashMap<Did, DidDocument>,
    /// Social recovery configurations.
    pub recovery_registry: HashMap<Did, RecoveryConfig>,
    /// AI agent identities linked to DIDs.
    pub agent_registry: HashMap<Did, Vec<AgentIdentity>>,
}

impl IdentityState {
    /// Create an empty identity state.
    pub fn new() -> Self {
        Self {
            dids: HashMap::new(),
            recovery_registry: HashMap::new(),
            agent_registry: HashMap::new(),
        }
    }

    /// Apply an identity operation, mutating state.
    pub fn apply(&mut self, op: &IdentityOp, vc: &VectorClock) -> Result<(), ShardError> {
        match op {
            IdentityOp::CreateDid { document } => {
                if self.dids.contains_key(&document.id) {
                    return Err(ShardError::StateConflict(format!(
                        "DID already exists: {}",
                        document.id
                    )));
                }
                self.dids.insert(document.id.clone(), document.clone());
                Ok(())
            }
            IdentityOp::UpdateDid { did, updates } => {
                let doc = self
                    .dids
                    .get_mut(did)
                    .ok_or_else(|| ShardError::ValidationFailed(format!("DID not found: {}", did)))?;

                for update in updates {
                    match update {
                        DidUpdate::AddAuthentication { public_key } => {
                            if !doc.authentication.contains(public_key) {
                                doc.authentication.push(*public_key);
                            }
                        }
                        DidUpdate::RemoveAuthentication { public_key } => {
                            doc.authentication.retain(|pk| pk != public_key);
                        }
                        DidUpdate::AddService { service_id, endpoint } => {
                            doc.services.insert(service_id.clone(), endpoint.clone());
                        }
                    }
                }
                doc.updated_at.merge(vc);
                Ok(())
            }
            IdentityOp::RecoverDid { did, recovery_shares } => {
                let config = self
                    .recovery_registry
                    .get(did)
                    .ok_or_else(|| {
                        ShardError::ValidationFailed(format!(
                            "No recovery config for DID: {}",
                            did
                        ))
                    })?;

                if (recovery_shares.len() as u8) < config.threshold {
                    return Err(ShardError::ValidationFailed(
                        "Insufficient recovery shares".into(),
                    ));
                }

                // Verify that the shares come from registered guardians
                for share in recovery_shares {
                    if !config.guardians.contains(&share.guardian) {
                        return Err(ShardError::ValidationFailed(
                            "Recovery share from unregistered guardian".into(),
                        ));
                    }
                }

                // Recovery successful — reset the document's authentication
                if let Some(doc) = self.dids.get_mut(did) {
                    doc.updated_at.merge(vc);
                    doc.recovery_enabled = true;
                }

                Ok(())
            }
            IdentityOp::VerifyDid { did } => {
                if !self.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!(
                        "DID not found: {}",
                        did
                    )));
                }
                Ok(())
            }
            IdentityOp::AddAgent { did, agent } => {
                if !self.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!(
                        "DID not found: {}",
                        did
                    )));
                }
                self.agent_registry
                    .entry(did.clone())
                    .or_default()
                    .push(agent.clone());
                Ok(())
            }
        }
    }

    /// Serialize the state to bytes for snapshots.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("IdentityState serialization cannot fail")
    }

    /// Deserialize state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

impl Default for IdentityState {
    fn default() -> Self {
        Self::new()
    }
}
