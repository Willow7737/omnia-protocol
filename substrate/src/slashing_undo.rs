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
use crate::vector_clock::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

/// A permanent audit record of a slashing undo that was applied.
///
/// Every undo operation is recorded for transparency and accountability.
/// These records cannot be deleted and are available for audit queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingUndoRecord {
    /// The validator whose slash was reversed.
    pub validator_id: NodeId,
    /// The governance proposal hash that authorized the undo.
    pub proposal_hash: [u8; 32],
    /// Slash points before the undo.
    pub points_before: u64,
    /// Slash points after the undo.
    pub points_after: u64,
    /// The round at which the original slash was recorded.
    pub slashed_round: u64,
    /// The round at which the undo was applied.
    pub undo_round: u64,
    /// Timestamp of the undo (unix epoch seconds).
    pub timestamp: u64,
    /// The reason for the undo.
    pub reason: String,
}

/// Manager for governance-based slashing undo operations.
///
/// Tracks undo requests and applies them to the slashing engine. Maintains
/// an audit log of all applied undos and enforces rate limits.
///
/// # Example
///
/// ```
/// use omnia_substrate::slashing_undo::{SlashingUndoManager, SlashingUndoRequest};
/// use omnia_substrate::slashing::{SlashingEngine, SlashOffense};
///
/// let mut slashing = SlashingEngine::new_in_memory(500, 2000);
/// let mut undo_mgr = SlashingUndoManager::new();
///
/// let mut node = [0u8; 32];
/// node[0] = 42;
///
/// // Record an offense
/// slashing.register_validator(node, 10_000);
/// slashing.record_offense(node, SlashOffense::LivenessViolation);
/// assert_eq!(slashing.slash_points_of(&node), 100);
///
/// // Governance undoes the slash
/// let request = SlashingUndoRequest {
///     validator_id: node,
///     slashed_round: 50,
///     undo_round: 100,
///     proposal_hash: [1u8; 32],
///     reason: "Software bug caused false positive".to_string(),
/// };
///
/// let record = undo_mgr.apply_undo(&mut slashing, request, 100).unwrap();
/// assert_eq!(slashing.slash_points_of(&node), 0);
/// assert_eq!(undo_mgr.audit_log().len(), 1);
/// ```
pub struct SlashingUndoManager {
    /// Audit log of all applied undos.
    audit_log: Vec<SlashingUndoRecord>,
    /// Rate-limiting: maps validator → last undo round.
    last_undo_round: HashMap<NodeId, u64>,
    /// Minimum round interval between undos for the same validator.
    min_undo_interval: u64,
}

/// Minimum number of rounds between successive undos for the same validator.
const DEFAULT_MIN_UNDO_INTERVAL: u64 = 1000;

impl SlashingUndoManager {
    /// Create a new undo manager with the default rate-limiting interval.
    pub fn new() -> Self {
        Self {
            audit_log: Vec::new(),
            last_undo_round: HashMap::new(),
            min_undo_interval: DEFAULT_MIN_UNDO_INTERVAL,
        }
    }

    /// Create a new undo manager with a custom rate-limiting interval.
    ///
    /// # Arguments
    ///
    /// * `min_undo_interval` — Minimum number of rounds between successive
    ///   undos for the same validator.
    pub fn with_interval(min_undo_interval: u64) -> Self {
        Self {
            audit_log: Vec::new(),
            last_undo_round: HashMap::new(),
            min_undo_interval,
        }
    }

    /// Apply a slashing undo request.
    ///
    /// Decrements the target validator's slash points by the amount of
    /// the last recorded offense (tracked via offense history) and records the undo in the
    /// audit log. Enforces the rate limit — at most one undo per validator
    /// per `min_undo_interval` rounds.
    ///
    /// # Arguments
    ///
    /// * `slashing` — The [`SlashingEngine`] to apply the undo to.
    /// * `request` — The [`SlashingUndoRequest`] from governance.
    /// * `current_round` — The current consensus round (for rate limiting).
    ///
    /// # Returns
    ///
    /// A [`SlashingUndoRecord`] on success, or an error string on failure.
    ///
    /// # Errors
    ///
    /// - Rate limit exceeded: too soon since last undo for this validator.
    /// - The validator has no offense history to undo.
    pub fn apply_undo(
        &mut self,
        slashing: &mut SlashingEngine,
        request: SlashingUndoRequest,
        current_round: u64,
    ) -> Result<SlashingUndoRecord, String> {
        // Rate limit check
        if let Some(&last_round) = self.last_undo_round.get(&request.validator_id) {
            if current_round.saturating_sub(last_round) < self.min_undo_interval {
                return Err(format!(
                    "Rate limit: last undo for validator {:?} was at round {}, current round {}, minimum interval {}",
                    &request.validator_id[..4],
                    last_round,
                    current_round,
                    self.min_undo_interval
                ));
            }
        }

        let points_before = slashing.slash_points_of(&request.validator_id);

        // Apply the undo via the slashing engine
        slashing.undo_slash(&request.validator_id)?;

        let points_after = slashing.slash_points_of(&request.validator_id);

        let record = SlashingUndoRecord {
            validator_id: request.validator_id,
            proposal_hash: request.proposal_hash,
            points_before,
            points_after,
            slashed_round: request.slashed_round,
            undo_round: request.undo_round,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            reason: request.reason,
        };

        // Update rate-limiting state
        self.last_undo_round
            .insert(request.validator_id, current_round);

        // Record in audit log
        self.audit_log.push(record.clone());

        tracing::info!(
            validator = ?&request.validator_id[..4],
            points_before,
            points_after,
            proposal = ?&request.proposal_hash[..4],
            "Slashing undo applied"
        );

        Ok(record)
    }

    /// Get the full audit log of all applied undos.
    pub fn audit_log(&self) -> &[SlashingUndoRecord] {
        &self.audit_log
    }

    /// Get all undo records for a specific validator.
    ///
    /// This is the history method required by the spec, returning all
    /// undo records for a given validator ID.
    pub fn history(&self, validator_id: &NodeId) -> Vec<&SlashingUndoRecord> {
        self.audit_log
            .iter()
            .filter(|r| &r.validator_id == validator_id)
            .collect()
    }

    /// Get all undo records for a specific validator (alias for `history`).
    ///
    /// Kept for backward compatibility.
    pub fn undo_history(&self, node: &NodeId) -> Vec<&SlashingUndoRecord> {
        self.history(node)
    }

    /// Check whether an undo is currently allowed for the given validator at the
    /// given round (i.e., the rate limit has not been exceeded).
    pub fn can_undo(&self, validator_id: &NodeId, current_round: u64) -> bool {
        if let Some(&last_round) = self.last_undo_round.get(validator_id) {
            current_round.saturating_sub(last_round) >= self.min_undo_interval
        } else {
            true
        }
    }

    /// Get the total number of undos that have been applied.
    pub fn total_undos(&self) -> usize {
        self.audit_log.len()
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

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    fn make_request(validator: NodeId, slashed_round: u64, undo_round: u64) -> SlashingUndoRequest {
        SlashingUndoRequest {
            validator_id: validator,
            slashed_round,
            undo_round,
            proposal_hash: [1u8; 32],
            reason: "Test undo".to_string(),
        }
    }

    #[test]
    fn test_undo_liveness_violation() {
        let mut slashing =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::new();

        let n = node(1);
        slashing.register_validator(n, 10_000);
        slashing.record_offense(n, SlashOffense::LivenessViolation);
        assert_eq!(slashing.slash_points_of(&n), 100);

        let request = make_request(n, 50, 100);
        let record = undo_mgr.apply_undo(&mut slashing, request, 100).unwrap();
        assert_eq!(record.points_before, 100);
        assert_eq!(record.points_after, 0);
        assert_eq!(record.slashed_round, 50);
        assert_eq!(record.undo_round, 100);
        assert_eq!(slashing.slash_points_of(&n), 0);
        assert_eq!(undo_mgr.total_undos(), 1);
    }

    #[test]
    fn test_undo_equivocation() {
        let mut slashing =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::new();

        let n = node(2);
        slashing.register_validator(n, 10_000);
        // Equivocation = 500 points
        slashing.record_offense(n, SlashOffense::Equivocation);
        assert_eq!(slashing.slash_points_of(&n), 500);

        // Undo now correctly removes the full equivocation amount (500 pts)
        let request = make_request(n, 50, 100);
        undo_mgr.apply_undo(&mut slashing, request, 100).unwrap();
        assert_eq!(slashing.slash_points_of(&n), 0);
    }

    #[test]
    fn test_undo_rate_limit() {
        let mut slashing =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::with_interval(100);

        let n = node(3);
        slashing.register_validator(n, 10_000);
        // Record two offenses so we have enough history for two undos
        slashing.record_offense(n, SlashOffense::Equivocation);
        slashing.record_offense(n, SlashOffense::LivenessViolation);

        // First undo at round 100
        let request1 = make_request(n, 50, 100);
        undo_mgr.apply_undo(&mut slashing, request1, 100).unwrap();

        // Second undo at round 150 — should fail (interval = 100)
        let request2 = make_request(n, 50, 150);
        let result = undo_mgr.apply_undo(&mut slashing, request2, 150);
        assert!(result.is_err());

        // Third undo at round 200 — should succeed (still has one offense in history)
        let request3 = make_request(n, 50, 200);
        undo_mgr.apply_undo(&mut slashing, request3, 200).unwrap();
        assert_eq!(undo_mgr.total_undos(), 2);
    }

    #[test]
    fn test_can_undo_rate_limit() {
        let mut undo_mgr = SlashingUndoManager::with_interval(100);
        let n = node(4);

        assert!(undo_mgr.can_undo(&n, 0));
        undo_mgr.last_undo_round.insert(n, 50);
        assert!(!undo_mgr.can_undo(&n, 100)); // 100 - 50 = 50 < 100
        assert!(undo_mgr.can_undo(&n, 150)); // 150 - 50 = 100 >= 100
    }

    #[test]
    fn test_undo_no_slash_points() {
        let mut slashing =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::new();

        let n = node(5);
        slashing.register_validator(n, 10_000);
        // No offense recorded — slash points = 0

        let request = make_request(n, 50, 100);
        let result = undo_mgr.apply_undo(&mut slashing, request, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_audit_log() {
        let mut slashing =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut undo_mgr = SlashingUndoManager::new();

        let n1 = node(10);
        let n2 = node(20);

        slashing.register_validator(n1, 10_000);
        slashing.register_validator(n2, 10_000);
        slashing.record_offense(n1, SlashOffense::LivenessViolation);
        slashing.record_offense(n2, SlashOffense::LivenessViolation);

        undo_mgr
            .apply_undo(&mut slashing, make_request(n1, 50, 100), 100)
            .unwrap();
        undo_mgr
            .apply_undo(&mut slashing, make_request(n2, 50, 100), 100)
            .unwrap();

        assert_eq!(undo_mgr.audit_log().len(), 2);
        assert_eq!(undo_mgr.history(&n1).len(), 1);
        assert_eq!(undo_mgr.history(&n2).len(), 1);
    }

    #[test]
    fn test_default_undo_manager() {
        let mgr = SlashingUndoManager::default();
        assert!(mgr.audit_log().is_empty());
        assert_eq!(mgr.total_undos(), 0);
    }

    #[test]
    fn test_slashing_undo_request_fields() {
        let request = SlashingUndoRequest {
            validator_id: node(42),
            slashed_round: 50,
            undo_round: 100,
            proposal_hash: [2u8; 32],
            reason: "Bug fix".to_string(),
        };
        assert_eq!(request.validator_id[0], 42);
        assert_eq!(request.slashed_round, 50);
        assert_eq!(request.undo_round, 100);
        assert_eq!(request.proposal_hash, [2u8; 32]);
        assert_eq!(request.reason, "Bug fix");
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
}
