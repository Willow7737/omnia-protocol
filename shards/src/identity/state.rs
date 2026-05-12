//! Identity shard state
//!
//! Maintains the DID document registry, recovery configurations,
//! biometric anchors, and agent registry. DID documents use
//! last-write-wins semantics backed by vector clocks for deterministic
//! resolution of concurrent updates.
//!
//! Extended in Layer 4 with:
//! - Shamir's Secret Sharing for social recovery
//! - Privacy-preserving biometric anchors
//! - AI agent identity with capability-based access control

use std::collections::HashMap;

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use super::agent::AgentIdentity;
use super::biometric::BiometricAnchor;
use super::ops::{Did, DidUpdate, IdentityOp};
use super::recovery::{RecoveryShare, ShamirRecovery};
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
    // TODO: Recovery should add the new recovered key to authentication
    // and optionally remove the compromised old key. Currently untouched.
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
    /// Minimum number of shares required for recovery (K in K-of-N).
    pub threshold: u8,
    /// Total number of shares created (N in K-of-N).
    pub total_shares: u8,
}

/// The full state of the Identity shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityState {
    /// DID document registry — maps DID strings to their documents.
    pub dids: HashMap<Did, DidDocument>,
    /// Social recovery configurations (Shamir's Secret Sharing).
    pub recovery_registry: HashMap<Did, RecoveryConfig>,
    /// AI agent identities keyed by agent DID.
    pub agent_registry: HashMap<Did, AgentIdentity>,
    /// Biometric anchors keyed by DID.
    pub biometric_registry: HashMap<Did, BiometricAnchor>,
}

impl IdentityState {
    /// Create an empty identity state.
    pub fn new() -> Self {
        Self {
            dids: HashMap::new(),
            recovery_registry: HashMap::new(),
            agent_registry: HashMap::new(),
            biometric_registry: HashMap::new(),
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
                    .ok_or_else(|| {
                        ShardError::ValidationFailed(format!("DID not found: {}", did))
                    })?;

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
                        DidUpdate::AddService {
                            service_id,
                            endpoint,
                        } => {
                            doc.services.insert(service_id.clone(), endpoint.clone());
                        }
                    }
                }
                doc.updated_at.merge(vc);
                Ok(())
            }
            IdentityOp::RecoverDid { did, shares } => {
                let config = self.recovery_registry.get(did).ok_or_else(|| {
                    ShardError::ValidationFailed(format!(
                        "No recovery config for DID: {}",
                        did
                    ))
                })?;

                if shares.len() < config.threshold as usize {
                    return Err(ShardError::ValidationFailed(
                        "Insufficient recovery shares".into(),
                    ));
                }

                // TODO: The reconstructed secret should be used to generate a new keypair
                // and rotate the DID's public_key and authentication list. Currently
                // we only validate the shares and set recovery_enabled = true.
                // Production fix: derive new key from reconstructed secret, update
                // doc.public_key and doc.authentication, invalidate old key.
                let _reconstructed = ShamirRecovery::reconstruct(shares).ok_or_else(|| {
                    ShardError::ValidationFailed("Recovery reconstruction failed".into())
                })?;

                // Recovery successful — update the document
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
                        "Owner DID not found: {}",
                        did
                    )));
                }
                if self.agent_registry.contains_key(&agent.did) {
                    return Err(ShardError::StateConflict(format!(
                        "Agent already exists: {}",
                        agent.did
                    )));
                }
                self.agent_registry
                    .insert(agent.did.clone(), agent.clone());
                Ok(())
            }
            IdentityOp::EnrollBiometric {
                did,
                template,
                algorithm,
            } => {
                if !self.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!(
                        "DID not found: {}",
                        did
                    )));
                }
                let anchor = BiometricAnchor::enroll(template, algorithm);
                self.biometric_registry.insert(did.clone(), anchor);
                Ok(())
            }
            IdentityOp::VerifyBiometric { did, template } => {
                let anchor = self.biometric_registry.get(did).ok_or_else(|| {
                    ShardError::ValidationFailed(format!(
                        "No biometric enrolled for DID: {}",
                        did
                    ))
                })?;
                if !anchor.verify(template) {
                    return Err(ShardError::ValidationFailed(
                        "Biometric verification failed".into(),
                    ));
                }
                Ok(())
            }
            IdentityOp::RevokeAgent { agent_did } => {
                let agent = self.agent_registry.get_mut(agent_did).ok_or_else(|| {
                    ShardError::ValidationFailed(format!(
                        "Agent not found: {}",
                        agent_did
                    ))
                })?;
                agent.revoke();
                Ok(())
            }
            IdentityOp::ConfigureRecovery {
                did,
                secret,
                threshold,
                total_shares,
            } => {
                if !self.dids.contains_key(did) {
                    return Err(ShardError::ValidationFailed(format!(
                        "DID not found: {}",
                        did
                    )));
                }
                let shares = ShamirRecovery::split(secret, *threshold, *total_shares);
                self.recovery_registry.insert(
                    did.clone(),
                    RecoveryConfig {
                        threshold: *threshold,
                        total_shares: *total_shares,
                    },
                );
                // TODO: Shares are generated but immediately dropped. In production,
                // these must be encrypted and distributed to guardians off-chain.
                // Each guardian receives one share. No single guardian can reconstruct
                // the secret alone. Consider: encrypt share_i with guardian_i's public
                // key and send via secure channel (not the causal graph).
                let _ = shares; // Shares are returned to the caller via a separate API
                Ok(())
            }
        }
    }

    /// Enroll a biometric anchor for a DID.
    ///
    /// Convenience method that creates a `BiometricAnchor` without going
    /// through the `apply` pipeline.
    pub fn enroll_biometric(
        &mut self,
        did: &str,
        template: &[u8],
        algorithm: &str,
    ) -> Result<(), ShardError> {
        if !self.dids.contains_key(did) {
            return Err(ShardError::ValidationFailed(format!(
                "DID not found: {}",
                did
            )));
        }
        let anchor = BiometricAnchor::enroll(template, algorithm);
        self.biometric_registry.insert(did.to_string(), anchor);
        Ok(())
    }

    /// Verify a biometric template against the stored commitment.
    pub fn verify_biometric(
        &self,
        did: &str,
        fresh_template: &[u8],
    ) -> Result<bool, ShardError> {
        let anchor = self.biometric_registry.get(did).ok_or_else(|| {
            ShardError::ValidationFailed(format!(
                "No biometric enrolled for DID: {}",
                did
            ))
        })?;
        Ok(anchor.verify(fresh_template))
    }

    /// Create Shamir's Secret Sharing recovery shares for a DID.
    pub fn create_recovery_shares(
        &mut self,
        did: &str,
        secret: &[u8],
        threshold: u8,
        total: u8,
    ) -> Result<Vec<RecoveryShare>, ShardError> {
        if !self.dids.contains_key(did) {
            return Err(ShardError::ValidationFailed(format!(
                "DID not found: {}",
                did
            )));
        }
        let shares = ShamirRecovery::split(secret, threshold, total);
        self.recovery_registry.insert(
            did.to_string(),
            RecoveryConfig {
                threshold,
                total_shares: total,
            },
        );
        Ok(shares)
    }

    /// Recover a DID secret using Shamir's Secret Sharing.
    pub fn recover_did(
        &self,
        did: &str,
        shares: &[RecoveryShare],
    ) -> Result<Vec<u8>, ShardError> {
        let config = self.recovery_registry.get(did).ok_or_else(|| {
            ShardError::ValidationFailed(format!(
                "No recovery config for DID: {}",
                did
            ))
        })?;
        if shares.len() < config.threshold as usize {
            return Err(ShardError::ValidationFailed(format!(
                "Insufficient shares: have {}, need {}",
                shares.len(),
                config.threshold
            )));
        }
        ShamirRecovery::reconstruct(shares).ok_or_else(|| {
            ShardError::ValidationFailed("Recovery reconstruction failed".into())
        })
    }

    /// Register an AI agent identity.
    pub fn register_agent(
        &mut self,
        agent: AgentIdentity,
    ) -> Result<(), ShardError> {
        if self.agent_registry.contains_key(&agent.did) {
            return Err(ShardError::StateConflict(format!(
                "Agent already exists: {}",
                agent.did
            )));
        }
        if !self.dids.contains_key(&agent.owner_did) {
            return Err(ShardError::ValidationFailed(format!(
                "Owner DID not found: {}",
                agent.owner_did
            )));
        }
        self.agent_registry.insert(agent.did.clone(), agent);
        Ok(())
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
