//! Governance-Based Slashing Undo.
//!
//! This module provides a mechanism for governance to reverse slashing
//! decisions that were made in error (e.g., due to a software bug,
//! misconfigured thresholds, or a coordinated false-accusation attack).
//!
//! # Motivation
//!
//! Slashing is irreversible by design — once stake is forfeited, it cannot
//! be recovered. However, there are legitimate scenarios where slashing
//! should be undone:
//!
//! - **Software bug**: A consensus bug caused false equivocation detections.
//! - **Misconfiguration**: Slash thresholds were set too low, causing
//!   unwarranted penalties.
//! - **Coordinated attack**: A cartel of validators framed an honest node.
//!
//! # Process
//!
//! 1. A governance proposal creates a [`SlashingUndoRequest`].
//! 2. If the proposal passes (supermajority vote), the request is applied.
//! 3. The [`SlashingUndoManager`] decrements the node's slash points and
//!    records the undo in the [`SlashingUndoRecord`] for auditability.
//!
//! # Constraints
//!
//! - Undo is rate-limited: at most one undo per node per 1000 blocks.
//! - Undo only decrements points by the amount of the last recorded offense
//!   per request, preventing a governance takeover from fully clearing
//!   a genuinely malicious validator in a single operation.
//! - All undos are permanently recorded in the audit log.

use crate::slashing::SlashingEngine;
use omnia_primitives::NodeId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// AUDIT-2026-07 C6 (#344): the audit record now lives in `slashing` so it
// can be persisted inside `SlashingState`. Re-exported here for the
// existing public path `slashing_undo::SlashingUndoRecord`.
pub use crate::slashing::SlashingUndoRecord;

/// Domain separator for the governance-undo authorization commitment.
const UNDO_COMMITMENT_DOMAIN: &[u8] = b"OMNIA-SLASHING-UNDO-GOV-V1";

/// Errors that can occur during governance-based slashing undo operations.
#[derive(Error, Debug)]
pub enum SlashingUndoError {
    /// The undo request was not authorized by a governance quorum
    /// (AUDIT-2026-07 C6, #344).
    #[error("governance authorization failed: {0}")]
    Unauthorized(String),
    /// The rate limit for undos on this validator has been exceeded.
    #[error("rate limit: last undo for validator {validator_prefix:?} was at round {last_round}, current round {current_round}, minimum interval {min_interval}", last_round = last_round, current_round = current_round, min_interval = min_interval)]
    RateLimitExceeded {
        /// First 4 bytes of the validator ID (for display).
        validator_prefix: [u8; 4],
        /// Round of the last undo for this validator.
        last_round: u64,
        /// Current consensus round.
        current_round: u64,
        /// Minimum interval between undos.
        min_interval: u64,
    },
    /// The validator has no offense history to undo.
    #[error("validator {prefix:?} has no offense history to undo", prefix = .0)]
    NoOffenseHistory([u8; 4]),
    /// The underlying slashing engine failed to undo the slash.
    #[error("slashing undo failed: {0}")]
    UndoFailed(#[from] crate::slashing::SlashingStoreError),
}

/// A request to reverse a slashing decision for a specific validator.
///
/// Created by governance when a slashing decision is determined to be
/// erroneous. Each request targets a single validator and includes the
/// round at which the original slash occurred and the round at which the
/// undo is being requested.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingUndoRequest {
    /// The validator whose slash should be partially reversed.
    pub validator_id: NodeId,
    /// The consensus round in which the original slash was recorded.
    pub slashed_round: u64,
    /// The consensus round at which the undo is being requested.
    pub undo_round: u64,
    /// The hash of the governance proposal that authorized this undo.
    pub proposal_hash: [u8; 32],
    /// A human-readable reason for the undo.
    pub reason: String,
    /// AUDIT-2026-07 C6 (#344): governance authorization — signatures by
    /// registered governance keys over [`authorization_commitment`]. The
    /// undo is applied only if at least the [`GovernanceAuthority`]'s
    /// threshold of distinct valid signatures is present. Without this the
    /// old code applied any undo from anyone with API access.
    ///
    /// [`authorization_commitment`]: SlashingUndoRequest::authorization_commitment
    #[serde(default)]
    pub governance_signatures: Vec<GovernanceSignature>,
}

/// A single governance signature over a slashing-undo authorization
/// commitment (AUDIT-2026-07 C6, #344).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceSignature {
    /// Index of the signing key in the [`GovernanceAuthority`] key set.
    pub key_index: u32,
    /// The 64-byte Ed25519 signature over the authorization commitment.
    pub signature: Vec<u8>,
}

impl SlashingUndoRequest {
    /// The message governance keys sign to authorize this undo.
    ///
    /// Binds the target validator, the slashed and undo rounds, the
    /// proposal hash, and the reason — so a signature for one undo cannot
    /// be replayed to authorize a different one.
    pub fn authorization_commitment(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(UNDO_COMMITMENT_DOMAIN);
        hasher.update(&self.validator_id);
        hasher.update(&self.slashed_round.to_le_bytes());
        hasher.update(&self.undo_round.to_le_bytes());
        hasher.update(&self.proposal_hash);
        hasher.update(&(self.reason.len() as u64).to_le_bytes());
        hasher.update(self.reason.as_bytes());
        *hasher.finalize().as_bytes()
    }
}

/// The registered set of governance keys and the quorum threshold that
/// must sign a slashing-undo request (AUDIT-2026-07 C6, #344).
///
/// This is a node-operator/genesis configuration — the governance key set
/// is trusted state, established out of band, not settable from a
/// consensus payload. In production the keys are the governance council's
/// public keys (or a threshold-BLS group), and the threshold is the
/// council's supermajority.
#[derive(Debug, Clone)]
pub struct GovernanceAuthority {
    keys: Vec<[u8; 32]>,
    threshold: usize,
}

impl GovernanceAuthority {
    /// Create a governance authority from a set of Ed25519 verifying keys
    /// and a signing threshold (`1 <= threshold <= keys.len()`).
    pub fn new(keys: Vec<[u8; 32]>, threshold: usize) -> Result<Self, SlashingUndoError> {
        if keys.is_empty() || threshold == 0 || threshold > keys.len() {
            return Err(SlashingUndoError::Unauthorized(format!(
                "invalid governance authority: {} keys, threshold {threshold}",
                keys.len()
            )));
        }
        Ok(Self { keys, threshold })
    }

    /// Verify that `signatures` contain at least `threshold` valid,
    /// distinct-key Ed25519 signatures over `commitment`.
    pub fn verify(&self, commitment: &[u8; 32], signatures: &[GovernanceSignature]) -> bool {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let mut seen = std::collections::HashSet::new();
        let mut valid = 0usize;
        for gs in signatures {
            let idx = gs.key_index as usize;
            if idx >= self.keys.len() || !seen.insert(idx) {
                continue; // out of range, or a duplicate key — does not count
            }
            let sig = match gs
                .signature
                .as_slice()
                .try_into()
                .map(|b: [u8; 64]| Signature::from_bytes(&b))
            {
                Ok(s) => s,
                Err(_) => continue,
            };
            let pk = match VerifyingKey::from_bytes(&self.keys[idx]) {
                Ok(pk) => pk,
                Err(_) => continue,
            };
            if pk.verify(commitment, &sig).is_ok() {
                valid += 1;
                if valid >= self.threshold {
                    return true;
                }
            }
        }
        false
    }

    /// The configured quorum threshold.
    pub fn threshold(&self) -> usize {
        self.threshold
    }
}

/// Manager for governance-authorized, persistently-rate-limited slashing
/// undo operations (AUDIT-2026-07 C6, #344).
///
/// Authorization: an undo is applied only if its request carries at least
/// the [`GovernanceAuthority`] threshold of valid governance signatures.
/// A manager built without an authority rejects every undo (fail closed) —
/// the old code applied any undo from anyone with API access.
///
/// Rate limiting & audit: the last-undo round and the audit log live in the
/// [`SlashingEngine`]'s persisted state, so a restart cannot reset them and
/// bypass the one-undo-per-interval cap. This manager holds only policy
/// (the governance authority and the interval), no mutable undo state.
pub struct SlashingUndoManager {
    /// Governance keys + threshold that must authorize each undo. `None`
    /// means no undo can be authorized (fail closed).
    governance: Option<GovernanceAuthority>,
    /// Minimum round interval between undos for the same validator.
    min_undo_interval: u64,
}

/// Minimum number of rounds between successive undos for the same validator.
const DEFAULT_MIN_UNDO_INTERVAL: u64 = 1000;

impl SlashingUndoManager {
    /// Create an undo manager with **no** governance authority.
    ///
    /// Such a manager rejects every undo (fail closed). Use
    /// [`with_governance`](Self::with_governance) to authorize undos.
    pub fn new() -> Self {
        Self {
            governance: None,
            min_undo_interval: DEFAULT_MIN_UNDO_INTERVAL,
        }
    }

    /// Create an undo manager authorized by the given governance authority.
    pub fn with_governance(governance: GovernanceAuthority) -> Self {
        Self {
            governance: Some(governance),
            min_undo_interval: DEFAULT_MIN_UNDO_INTERVAL,
        }
    }

    /// Create an undo manager with a governance authority and a custom
    /// rate-limit interval.
    pub fn with_governance_and_interval(governance: GovernanceAuthority, min_undo_interval: u64) -> Self {
        Self {
            governance: Some(governance),
            min_undo_interval,
        }
    }

    /// Set (or replace) the governance authority.
    pub fn set_governance(&mut self, governance: GovernanceAuthority) {
        self.governance = Some(governance);
    }

    /// Apply a governance-authorized slashing undo.
    ///
    /// Verifies the request's governance signatures against the configured
    /// [`GovernanceAuthority`], enforces the persisted rate limit, then
    /// applies the undo and records it **atomically** via
    /// [`SlashingEngine::record_undo`] (rate-limit stamp + audit entry +
    /// point decrement in one persisted transaction).
    ///
    /// # Errors
    /// - [`SlashingUndoError::Unauthorized`] — no governance authority is
    ///   configured, or the request lacks a valid signing quorum.
    /// - [`SlashingUndoError::RateLimitExceeded`] — too soon since the last
    ///   undo for this validator (checked against persisted state).
    /// - [`SlashingUndoError::UndoFailed`] — the engine could not apply or
    ///   persist the undo.
    pub fn apply_undo(
        &mut self,
        slashing: &mut SlashingEngine,
        request: SlashingUndoRequest,
        current_round: u64,
    ) -> Result<SlashingUndoRecord, SlashingUndoError> {
        // 1. Governance authorization (AUDIT-2026-07 C6, #344).
        let authority = self.governance.as_ref().ok_or_else(|| {
            SlashingUndoError::Unauthorized("no governance authority configured — undo refused".to_string())
        })?;
        if !authority.verify(&request.authorization_commitment(), &request.governance_signatures) {
            return Err(SlashingUndoError::Unauthorized(format!(
                "insufficient valid governance signatures (need {})",
                authority.threshold()
            )));
        }

        // 2. Rate limit — read from PERSISTED state so restarts can't reset it.
        if let Some(last_round) = slashing.last_undo_round(&request.validator_id) {
            if current_round.saturating_sub(last_round) < self.min_undo_interval {
                let mut prefix = [0u8; 4];
                prefix.copy_from_slice(&request.validator_id[..4]);
                return Err(SlashingUndoError::RateLimitExceeded {
                    validator_prefix: prefix,
                    last_round,
                    current_round,
                    min_interval: self.min_undo_interval,
                });
            }
        }

        // 3. Apply + record atomically (persisted in one transaction).
        let record = slashing.record_undo(
            &request.validator_id,
            current_round,
            request.slashed_round,
            request.undo_round,
            request.proposal_hash,
            request.reason,
        )?;

        tracing::info!(
            validator = ?&record.validator_id[..4],
            points_before = record.points_before,
            points_after = record.points_after,
            proposal = ?&record.proposal_hash[..4],
            "Governance-authorized slashing undo applied"
        );
        Ok(record)
    }

    /// The full persisted audit log of applied undos.
    pub fn audit_log(&self, slashing: &SlashingEngine) -> Vec<SlashingUndoRecord> {
        slashing.undo_audit_log()
    }

    /// All undo records for a specific validator.
    pub fn history(&self, slashing: &SlashingEngine, validator_id: &NodeId) -> Vec<SlashingUndoRecord> {
        slashing
            .undo_audit_log()
            .into_iter()
            .filter(|r| &r.validator_id == validator_id)
            .collect()
    }

    /// Whether an undo is currently allowed for `validator_id` at
    /// `current_round` (rate limit not exceeded), per persisted state.
    pub fn can_undo(&self, slashing: &SlashingEngine, validator_id: &NodeId, current_round: u64) -> bool {
        match slashing.last_undo_round(validator_id) {
            Some(last_round) => current_round.saturating_sub(last_round) >= self.min_undo_interval,
            None => true,
        }
    }

    /// Total number of undos that have been applied (persisted).
    pub fn total_undos(&self, slashing: &SlashingEngine) -> usize {
        slashing.undo_audit_log().len()
    }
}

impl Default for SlashingUndoManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::slashing::{SlashOffense, DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD};
    use ed25519_dalek::{Signer, SigningKey};

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    /// A deterministic governance signing key for tests.
    fn gov_key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    /// A 1-of-1 governance authority for the given key.
    fn authority_of(key: &SigningKey) -> GovernanceAuthority {
        GovernanceAuthority::new(vec![key.verifying_key().to_bytes()], 1).unwrap()
    }

    /// A manager authorized by a single governance key with a given interval.
    fn governed(key: &SigningKey, interval: u64) -> SlashingUndoManager {
        SlashingUndoManager::with_governance_and_interval(authority_of(key), interval)
    }

    /// Build an undo request signed by `signers` (each at its key index).
    fn signed_request(
        validator: NodeId,
        slashed_round: u64,
        undo_round: u64,
        signers: &[(u32, &SigningKey)],
    ) -> SlashingUndoRequest {
        let mut request = SlashingUndoRequest {
            validator_id: validator,
            slashed_round,
            undo_round,
            proposal_hash: [1u8; 32],
            reason: "Test undo".to_string(),
            governance_signatures: vec![],
        };
        let commitment = request.authorization_commitment();
        request.governance_signatures = signers
            .iter()
            .map(|(idx, sk)| GovernanceSignature {
                key_index: *idx,
                signature: sk.sign(&commitment).to_bytes().to_vec(),
            })
            .collect();
        request
    }

    #[test]
    fn test_undo_liveness_violation() {
        let gov = gov_key(7);
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::with_governance(authority_of(&gov));

        let n = node(1);
        slashing.register_validator(n, 10_000);
        slashing.record_offense(n, SlashOffense::LivenessViolation);
        assert_eq!(slashing.slash_points_of(&n), 100);

        let request = signed_request(n, 50, 100, &[(0, &gov)]);
        let record = undo_mgr.apply_undo(&mut slashing, request, 100).unwrap();
        assert_eq!(record.points_before, 100);
        assert_eq!(record.points_after, 0);
        assert_eq!(slashing.slash_points_of(&n), 0);
        assert_eq!(undo_mgr.total_undos(&slashing), 1);
    }

    // ── AUDIT-2026-07 C6 (#344) regression tests ──────────────────────────

    #[test]
    fn undo_without_governance_authority_is_rejected() {
        // A manager with no authority must refuse every undo (fail closed).
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::new();
        let n = node(1);
        slashing.register_validator(n, 10_000);
        slashing.record_offense(n, SlashOffense::Equivocation);

        let gov = gov_key(7);
        let request = signed_request(n, 50, 100, &[(0, &gov)]);
        let err = undo_mgr.apply_undo(&mut slashing, request, 100).unwrap_err();
        assert!(matches!(err, SlashingUndoError::Unauthorized(_)));
        // The slash must be untouched.
        assert_eq!(slashing.slash_points_of(&n), 500);
    }

    #[test]
    fn undo_signed_by_wrong_key_is_rejected() {
        // THE C6 regression: the old code applied any undo. Now a signature
        // by a key that is NOT in the governance set is rejected.
        let gov = gov_key(7);
        let attacker = gov_key(66);
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::with_governance(authority_of(&gov));
        let n = node(1);
        slashing.register_validator(n, 10_000);
        slashing.record_offense(n, SlashOffense::Equivocation);

        // Attacker signs with their own key, claiming key index 0.
        let request = signed_request(n, 50, 100, &[(0, &attacker)]);
        let err = undo_mgr.apply_undo(&mut slashing, request, 100).unwrap_err();
        assert!(matches!(err, SlashingUndoError::Unauthorized(_)));
        assert_eq!(slashing.slash_points_of(&n), 500);
    }

    #[test]
    fn unsigned_undo_is_rejected() {
        let gov = gov_key(7);
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::with_governance(authority_of(&gov));
        let n = node(1);
        slashing.register_validator(n, 10_000);
        slashing.record_offense(n, SlashOffense::Equivocation);

        let request = signed_request(n, 50, 100, &[]); // no signatures
        assert!(matches!(
            undo_mgr.apply_undo(&mut slashing, request, 100),
            Err(SlashingUndoError::Unauthorized(_))
        ));
    }

    #[test]
    fn threshold_requires_multiple_signers() {
        // 2-of-3 governance: one signature is not enough.
        let k0 = gov_key(1);
        let k1 = gov_key(2);
        let k2 = gov_key(3);
        let authority = GovernanceAuthority::new(
            vec![
                k0.verifying_key().to_bytes(),
                k1.verifying_key().to_bytes(),
                k2.verifying_key().to_bytes(),
            ],
            2,
        )
        .unwrap();
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::with_governance(authority);
        let n = node(1);
        slashing.register_validator(n, 10_000);
        slashing.record_offense(n, SlashOffense::Equivocation);
        slashing.record_offense(n, SlashOffense::LivenessViolation);

        // One signer → rejected.
        let one = signed_request(n, 50, 100, &[(0, &k0)]);
        assert!(matches!(
            undo_mgr.apply_undo(&mut slashing, one, 100),
            Err(SlashingUndoError::Unauthorized(_))
        ));
        // Two distinct signers → accepted.
        let two = signed_request(n, 50, 100, &[(0, &k0), (2, &k2)]);
        assert!(undo_mgr.apply_undo(&mut slashing, two, 100).is_ok());
    }

    #[test]
    fn rate_limit_survives_a_restart() {
        // THE persistence regression: the rate-limit state must live in the
        // slashing store so reopening the engine does NOT reset it and let a
        // second undo through within the interval.
        use crate::slashing::{RedbSlashingStore, SlashingStore};
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slashing.redb");
        let gov = gov_key(7);
        let n = node(9);

        {
            let store: Arc<dyn SlashingStore> = Arc::new(RedbSlashingStore::open(&path).unwrap());
            let mut slashing = SlashingEngine::with_store(store).unwrap();
            slashing.register_validator(n, 10_000);
            slashing.record_offense(n, SlashOffense::Equivocation);
            slashing.record_offense(n, SlashOffense::LivenessViolation);
            let mut mgr = governed(&gov, 100);
            mgr.apply_undo(&mut slashing, signed_request(n, 50, 100, &[(0, &gov)]), 100)
                .unwrap();
        }

        // Reopen from the same file — the last-undo round must persist.
        let store: Arc<dyn SlashingStore> = Arc::new(RedbSlashingStore::open(&path).unwrap());
        let mut slashing = SlashingEngine::with_store(store).unwrap();
        assert_eq!(slashing.last_undo_round(&n), Some(100), "rate-limit round must persist");
        assert_eq!(slashing.undo_audit_log().len(), 1, "audit log must persist");

        let mut mgr = governed(&gov, 100);
        // A second undo at round 150 (interval 100) must still be rate-limited
        // AFTER the restart — the old in-memory state would have reset to allow it.
        let err = mgr
            .apply_undo(&mut slashing, signed_request(n, 50, 150, &[(0, &gov)]), 150)
            .unwrap_err();
        assert!(matches!(err, SlashingUndoError::RateLimitExceeded { .. }));
    }

    #[test]
    fn test_undo_rate_limit() {
        let gov = gov_key(7);
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = governed(&gov, 100);

        let n = node(3);
        slashing.register_validator(n, 10_000);
        slashing.record_offense(n, SlashOffense::Equivocation);
        slashing.record_offense(n, SlashOffense::LivenessViolation);

        undo_mgr
            .apply_undo(&mut slashing, signed_request(n, 50, 100, &[(0, &gov)]), 100)
            .unwrap();
        // Second undo at round 150 — within the interval, rejected.
        assert!(undo_mgr
            .apply_undo(&mut slashing, signed_request(n, 50, 150, &[(0, &gov)]), 150)
            .is_err());
        // Third at round 200 — interval elapsed, succeeds.
        undo_mgr
            .apply_undo(&mut slashing, signed_request(n, 50, 200, &[(0, &gov)]), 200)
            .unwrap();
        assert_eq!(undo_mgr.total_undos(&slashing), 2);
    }

    #[test]
    fn test_can_undo_rate_limit() {
        let gov = gov_key(7);
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = governed(&gov, 100);
        let n = node(4);
        slashing.register_validator(n, 10_000);
        slashing.record_offense(n, SlashOffense::Equivocation);

        assert!(undo_mgr.can_undo(&slashing, &n, 0));
        undo_mgr
            .apply_undo(&mut slashing, signed_request(n, 50, 50, &[(0, &gov)]), 50)
            .unwrap();
        assert!(!undo_mgr.can_undo(&slashing, &n, 100)); // 100 - 50 = 50 < 100
        assert!(undo_mgr.can_undo(&slashing, &n, 150)); // 150 - 50 = 100 >= 100
    }

    #[test]
    fn test_undo_no_slash_points() {
        let gov = gov_key(7);
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::with_governance(authority_of(&gov));

        let n = node(5);
        slashing.register_validator(n, 10_000);
        // No offense recorded — nothing to undo.
        let request = signed_request(n, 50, 100, &[(0, &gov)]);
        assert!(undo_mgr.apply_undo(&mut slashing, request, 100).is_err());
    }

    #[test]
    fn test_audit_log() {
        let gov = gov_key(7);
        let mut slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::with_governance(authority_of(&gov));

        let n1 = node(10);
        let n2 = node(20);
        slashing.register_validator(n1, 10_000);
        slashing.register_validator(n2, 10_000);
        slashing.record_offense(n1, SlashOffense::LivenessViolation);
        slashing.record_offense(n2, SlashOffense::LivenessViolation);

        undo_mgr
            .apply_undo(&mut slashing, signed_request(n1, 50, 100, &[(0, &gov)]), 100)
            .unwrap();
        undo_mgr
            .apply_undo(&mut slashing, signed_request(n2, 50, 100, &[(0, &gov)]), 100)
            .unwrap();

        assert_eq!(undo_mgr.audit_log(&slashing).len(), 2);
        assert_eq!(undo_mgr.history(&slashing, &n1).len(), 1);
        assert_eq!(undo_mgr.history(&slashing, &n2).len(), 1);
    }

    #[test]
    fn governance_authority_rejects_bad_config() {
        assert!(GovernanceAuthority::new(vec![], 1).is_err());
        assert!(GovernanceAuthority::new(vec![[1u8; 32]], 0).is_err());
        assert!(GovernanceAuthority::new(vec![[1u8; 32]], 2).is_err());
        assert!(GovernanceAuthority::new(vec![[1u8; 32]], 1).is_ok());
    }

    #[test]
    fn test_slashing_undo_record_fields() {
        let record = SlashingUndoRecord {
            validator_id: node(42),
            proposal_hash: [3u8; 32],
            points_before: 500,
            points_after: 400,
            slashed_round: 50,
            undo_round: 100,
            timestamp: 1234567890,
            reason: "Test".to_string(),
        };
        assert_eq!(record.validator_id[0], 42);
        assert_eq!(record.slashed_round, 50);
        assert_eq!(record.undo_round, 100);
    }

    #[test]
    fn test_slashing_undo_error_variants_display() {
        let e = SlashingUndoError::RateLimitExceeded {
            validator_prefix: [1, 2, 3, 4],
            last_round: 50,
            current_round: 75,
            min_interval: 100,
        };
        assert!(e.to_string().contains("rate limit"));

        let e = SlashingUndoError::NoOffenseHistory([1, 2, 3, 4]);
        assert!(e.to_string().contains("no offense history"));

        let store_err = crate::slashing::SlashingStoreError::Persistence("db error".to_string());
        let e = SlashingUndoError::UndoFailed(store_err);
        assert!(e.to_string().contains("slashing undo failed"));
    }

    #[test]
    fn test_slashing_undo_error_from_slashing_store_error() {
        let store_err = crate::slashing::SlashingStoreError::Serialization("serde fail".to_string());
        let undo_err: SlashingUndoError = store_err.into();
        assert!(matches!(undo_err, SlashingUndoError::UndoFailed(_)));
    }
}
