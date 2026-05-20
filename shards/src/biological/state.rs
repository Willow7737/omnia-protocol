//! Biological shard state
//!
//! Maintains the consent registry and data vault references. Consent is
//! modeled as a set of granted access records that can be revoked.

use std::collections::HashMap;

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use super::ops::BiologicalOp;
use crate::shard::ShardError;

/// A record of consent granted by a subject to a consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRecord {
    /// The subject who granted consent.
    pub subject: super::ops::SubjectId,
    /// The consumer who received consent.
    pub consumer: super::ops::ConsumerId,
    /// Scope of the consent (e.g., "lab-results").
    pub scope: String,
    /// When consent was granted (vector clock for ordering).
    pub granted_at: VectorClock,
    /// Expiration timestamp (0 = no expiry).
    pub expires_at: u64,
    /// Whether this consent has been revoked.
    pub revoked: bool,
}

impl ConsentRecord {
    /// Check if this consent record is currently active.
    pub fn is_active(&self, now: u64) -> bool {
        !self.revoked && (self.expires_at == 0 || self.expires_at > now)
    }
}

/// Key for the consent registry: (subject, consumer).
type ConsentKey = (super::ops::SubjectId, super::ops::ConsumerId);

/// The full state of the Biological shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiologicalState {
    /// Consent registry — maps (subject, consumer) pairs to their consent records.
    pub consent_registry: HashMap<ConsentKey, ConsentRecord>,
}

impl BiologicalState {
    /// Create an empty biological state.
    pub fn new() -> Self {
        Self {
            consent_registry: HashMap::new(),
        }
    }

    /// Apply a biological operation, mutating state.
    pub fn apply(&mut self, op: &BiologicalOp, vc: &VectorClock) -> Result<(), ShardError> {
        match op {
            BiologicalOp::GrantAccess {
                subject,
                consumer,
                scope,
                expires_at,
            } => {
                let key = (*subject, *consumer);
                self.consent_registry.insert(
                    key,
                    ConsentRecord {
                        subject: *subject,
                        consumer: *consumer,
                        scope: scope.clone(),
                        granted_at: vc.clone(),
                        expires_at: *expires_at,
                        revoked: false,
                    },
                );
                Ok(())
            }
            BiologicalOp::RevokeAccess { subject, consumer } => {
                let key = (*subject, *consumer);
                let record = self
                    .consent_registry
                    .get_mut(&key)
                    .ok_or_else(|| ShardError::ValidationFailed("Consent record not found".into()))?;
                record.revoked = true;
                Ok(())
            }
            BiologicalOp::QueryWithZkProof {
                subject,
                consumer,
                zk_proof,
                ..
            } => {
                let key = (*subject, *consumer);
                let record = self
                    .consent_registry
                    .get(&key)
                    .ok_or_else(|| ShardError::ValidationFailed("No consent for this query".into()))?;

                if record.revoked {
                    return Err(ShardError::ValidationFailed("Consent has been revoked".into()));
                }

                // In a real implementation, verify the ZK proof here.
                // For now, we accept the proof if consent exists.
                let _ = zk_proof;
                Ok(())
            }
        }
    }

    /// Serialize the state to bytes for snapshots.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    /// Deserialize state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

impl Default for BiologicalState {
    fn default() -> Self {
        Self::new()
    }
}
