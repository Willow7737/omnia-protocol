//! Biological shard validator
//!
//! Pre-flight validation for biological operations, including ZK-proof
//! validation stubs.

use super::ops::BiologicalOp;
use super::state::BiologicalState;
use crate::payload::ShardOp;
use crate::shard::ShardError;

/// Validator for the Biological shard.
pub struct BiologicalValidator;

impl BiologicalValidator {
    /// Validate a biological operation against the given state.
    pub fn validate(state: &BiologicalState, op: &BiologicalOp) -> Result<(), ShardError> {
        match op {
            BiologicalOp::GrantAccess {
                subject,
                consumer,
                scope,
                ..
            } => {
                if scope.is_empty() {
                    return Err(ShardError::InvalidOperation("Scope cannot be empty".into()));
                }
                let _ = (subject, consumer);
                Ok(())
            }
            BiologicalOp::RevokeAccess { subject, consumer } => {
                let key = (*subject, *consumer);
                if !state.consent_registry.contains_key(&key) {
                    return Err(ShardError::ValidationFailed("Consent record not found".into()));
                }
                Ok(())
            }
            BiologicalOp::QueryWithZkProof { subject, consumer, .. } => {
                let key = (*subject, *consumer);
                match state.consent_registry.get(&key) {
                    Some(record) if !record.revoked => Ok(()),
                    Some(_) => Err(ShardError::ValidationFailed("Consent has been revoked".into())),
                    None => Err(ShardError::ValidationFailed("No consent for this query".into())),
                }
            }
        }
    }

    /// Validate a `ShardOp::Biological` variant.
    pub fn validate_shard_op(state: &BiologicalState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Biological(bio_op) => Self::validate(state, bio_op),
            _ => Err(ShardError::InvalidOperation("Not a Biological operation".into())),
        }
    }
}
