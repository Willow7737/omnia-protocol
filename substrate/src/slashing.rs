//! Slashing Module for Byzantine Fault Detection
//!
//! This module implements a slashing engine that penalizes validators for
//! Byzantine behavior such as equivocation, liveness violations, and invalid
//! attestations. Slash points accumulate per offense, and when a node's
//! points exceed configurable thresholds, the node is either slashed (stake
//! forfeited) or ejected from the validator set entirely.
//!
//! # Offense Points
//!
//! | Offense              | Points |
//! |----------------------|--------|
//! | Equivocation         | 500    |
//! | LivenessViolation    | 100    |
//! | InvalidAttestation   | 300    |
//!
//! # Thresholds
//!
//! - **Slash threshold** (default 500): Points at which a node is *slashed*
//!   (stake forfeited).
//! - **Ejection threshold** (default 2000): Points at which a node is
//!   *ejected* (removed from the validator set).
//!
//! All points and thresholds are `u64` integers — no floating-point arithmetic.

use crate::event::Event;
use crate::vector_clock::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default slash threshold: accumulated points at which a node is slashed.
pub const DEFAULT_SLASH_THRESHOLD: u64 = 500;

/// Default ejection threshold: accumulated points at which a node is ejected.
pub const DEFAULT_EJECTION_THRESHOLD: u64 = 2000;

/// Points assigned for an equivocation offense.
const EQUIVOCATION_POINTS: u64 = 500;

/// Points assigned for a liveness violation.
const LIVENESS_VIOLATION_POINTS: u64 = 100;

/// Points assigned for an invalid attestation.
const INVALID_ATTESTATION_POINTS: u64 = 300;

/// Categorizes the type of Byzantine offense committed by a validator.
///
/// Each offense type carries a fixed penalty in slash points:
/// - [`SlashOffense::Equivocation`]: 500 points
/// - [`SlashOffense::LivenessViolation`]: 100 points
/// - [`SlashOffense::InvalidAttestation`]: 300 points
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashOffense {
    /// A validator signed two different events with the same creator and
    /// sequence number (double-signing / equivocation).
    Equivocation,
    /// A validator has been offline or unresponsive for too many rounds.
    LivenessViolation,
    /// A validator attested to invalid or fraudulent data.
    InvalidAttestation,
}

impl SlashOffense {
    /// Returns the number of slash points assigned to this offense type.
    ///
    /// # Returns
    ///
    /// A `u64` representing the penalty in slash points.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::SlashOffense;
    /// assert_eq!(SlashOffense::Equivocation.points(), 500);
    /// assert_eq!(SlashOffense::LivenessViolation.points(), 100);
    /// assert_eq!(SlashOffense::InvalidAttestation.points(), 300);
    /// ```
    pub fn points(&self) -> u64 {
        match self {
            SlashOffense::Equivocation => EQUIVOCATION_POINTS,
            SlashOffense::LivenessViolation => LIVENESS_VIOLATION_POINTS,
            SlashOffense::InvalidAttestation => INVALID_ATTESTATION_POINTS,
        }
    }
}

/// Describes the outcome of recording a slashing offense.
///
/// The outcome depends on the node's total accumulated slash points relative
/// to the configured thresholds:
/// - Below slash threshold → [`SlashOutcome::Warned`]
/// - At or above slash threshold but below ejection threshold → [`SlashOutcome::Slashed`]
/// - At or above ejection threshold → [`SlashOutcome::Ejected`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashOutcome {
    /// Node received slash points but is still below the slash threshold.
    Warned {
        /// The offending node.
        node: NodeId,
        /// Total accumulated slash points after this offense.
        points: u64,
    },
    /// Node has accumulated enough points to be slashed (stake forfeited).
    Slashed {
        /// The offending node.
        node: NodeId,
        /// Amount of stake being slashed.
        amount: u64,
    },
    /// Node has accumulated enough points to be ejected from the validator set.
    Ejected {
        /// The ejected node.
        node: NodeId,
    },
}

/// Engine that tracks slash points, stakes, and thresholds for validator
/// slashing.
///
/// The `SlashingEngine` is responsible for:
/// - Registering validators with their initial stake
/// - Recording offenses and accumulating slash points
/// - Determining slash outcomes based on accumulated points
/// - Detecting equivocation and liveness violations
///
/// # Example
///
/// ```
/// use omnia_substrate::{SlashingEngine, SlashOffense, SlashOutcome};
///
/// let mut engine = SlashingEngine::new(500, 2000);
/// let mut node = [0u8; 32];
/// node[0] = 42;
///
/// engine.register_validator(node, 10_000);
/// let outcome = engine.record_offense(node, SlashOffense::Equivocation);
/// assert!(matches!(outcome, SlashOutcome::Slashed { .. }));
/// ```
pub struct SlashingEngine {
    /// Accumulated slash points per node.
    slash_points: HashMap<NodeId, u64>,
    /// Staked amounts per node.
    stakes: HashMap<NodeId, u64>,
    /// Points threshold at which a node is slashed.
    slash_threshold: u64,
    /// Points threshold at which a node is ejected.
    ejection_threshold: u64,
}

impl Default for SlashingEngine {
    fn default() -> Self {
        Self::new(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD)
    }
}

impl SlashingEngine {
    /// Creates a new `SlashingEngine` with the given thresholds.
    ///
    /// # Arguments
    ///
    /// * `slash_threshold` — Slash points at which a node is considered
    ///   *slashed* (stake forfeited). Defaults to 500.
    /// * `ejection_threshold` — Slash points at which a node is *ejected*
    ///   from the validator set. Defaults to 2000.
    ///
    /// # Returns
    ///
    /// A new `SlashingEngine` instance with no registered validators.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::SlashingEngine;
    ///
    /// let engine = SlashingEngine::new(500, 2000);
    /// ```
    pub fn new(slash_threshold: u64, ejection_threshold: u64) -> Self {
        Self {
            slash_points: HashMap::new(),
            stakes: HashMap::new(),
            slash_threshold,
            ejection_threshold,
        }
    }

    /// Registers a validator with an initial stake.
    ///
    /// If the node is already registered, the stake is updated (replaced)
    /// with the new value.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` of the validator to register.
    /// * `stake` — The amount of stake the validator is bonding.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::SlashingEngine;
    ///
    /// let mut engine = SlashingEngine::new(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 1;
    ///
    /// engine.register_validator(node, 10_000);
    /// assert_eq!(engine.stake_of(&node), 10_000);
    /// ```
    pub fn register_validator(&mut self, node: NodeId, stake: u64) {
        tracing::info!(
            node = ?&node[..4],
            stake = stake,
            "Registering validator with stake"
        );
        self.stakes.insert(node, stake);
        // Ensure slash_points entry exists so slash_points_of returns 0
        // instead of implicitly missing.
        self.slash_points.entry(node).or_insert(0);
    }

    /// Records a slashing offense for a node and returns the resulting outcome.
    ///
    /// Slash points are accumulated. The outcome is determined by the total
    /// accumulated points relative to the configured thresholds:
    ///
    /// | Total points                  | Outcome   |
    /// |-------------------------------|-----------|
    /// | < slash_threshold             | Warned    |
    /// | ≥ slash_threshold, < ejection | Slashed   |
    /// | ≥ ejection_threshold          | Ejected   |
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` of the offending validator.
    /// * `offense` — The type of offense committed.
    ///
    /// # Returns
    ///
    /// A [`SlashOutcome`] indicating the consequence of this offense.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense, SlashOutcome};
    ///
    /// let mut engine = SlashingEngine::new(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 5;
    ///
    /// engine.register_validator(node, 10_000);
    /// let outcome = engine.record_offense(node, SlashOffense::LivenessViolation);
    /// assert!(matches!(outcome, SlashOutcome::Warned { .. }));
    /// ```
    pub fn record_offense(&mut self, node: NodeId, offense: SlashOffense) -> SlashOutcome {
        let points_added = offense.points();
        let current_points = self.slash_points.entry(node).or_insert(0);
        *current_points = current_points.saturating_add(points_added);
        let total_points = *current_points;

        tracing::warn!(
            node = ?&node[..4],
            offense = ?offense,
            points_added = points_added,
            total_points = total_points,
            "Slashing offense recorded"
        );

        if total_points >= self.ejection_threshold {
            tracing::info!(node = ?&node[..4], total_points, "Node ejected from consensus");
            SlashOutcome::Ejected { node }
        } else if total_points >= self.slash_threshold {
            let amount = self.stakes.get(&node).copied().unwrap_or(0);
            tracing::info!(
                node = ?&node[..4],
                total_points,
                amount,
                "Node slashed"
            );
            SlashOutcome::Slashed { node, amount }
        } else {
            tracing::debug!(
                node = ?&node[..4],
                total_points,
                threshold = self.slash_threshold,
                "Node warned — below slash threshold"
            );
            SlashOutcome::Warned {
                node,
                points: total_points,
            }
        }
    }

    /// Checks whether two events constitute an equivocation.
    ///
    /// Equivocation occurs when a node signs two *different* events that share
    /// the same `creator` and `sequence` number. This indicates the validator
    /// is creating conflicting histories.
    ///
    /// # Arguments
    ///
    /// * `event_a` — The first event to compare.
    /// * `event_b` — The second event to compare.
    ///
    /// # Returns
    ///
    /// `true` if both events have the same creator and sequence number but
    /// different `EventId`s (i.e., they are equivocating).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use omnia_substrate::SlashingEngine;
    /// // event_a and event_b have same creator & sequence, different hashes
    /// assert!(SlashingEngine::check_equivocation(&event_a, &event_b));
    /// ```
    pub fn check_equivocation(event_a: &Event, event_b: &Event) -> bool {
        event_a.creator == event_b.creator
            && event_a.sequence == event_b.sequence
            && event_a.id != event_b.id
    }

    /// Checks for a liveness violation and records it if detected.
    ///
    /// A liveness violation occurs when a node has been inactive for more
    /// than `threshold` rounds (i.e., `current_round - last_active_round > threshold`).
    /// If a violation is detected, a [`SlashOffense::LivenessViolation`]
    /// offense is recorded and the resulting [`SlashOutcome`] is returned.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` of the validator to check.
    /// * `last_active_round` — The last round in which the node participated.
    /// * `current_round` — The current consensus round.
    /// * `threshold` — The number of inactive rounds before a violation is triggered.
    ///
    /// # Returns
    ///
    /// `Some(SlashOutcome)` if a liveness violation was detected and recorded,
    /// `None` if the node is within the acceptable inactivity window.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOutcome};
    ///
    /// let mut engine = SlashingEngine::new(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 7;
    ///
    /// engine.register_validator(node, 5_000);
    ///
    /// // Node was last active at round 5, current round is 20, threshold is 10
    /// let result = engine.check_liveness(node, 5, 20, 10);
    /// assert!(result.is_some());
    /// ```
    pub fn check_liveness(
        &mut self,
        node: NodeId,
        last_active_round: u64,
        current_round: u64,
        threshold: u64,
    ) -> Option<SlashOutcome> {
        let inactive_rounds = current_round.saturating_sub(last_active_round);
        if inactive_rounds > threshold {
            tracing::info!(
                node = ?&node[..4],
                last_active_round,
                current_round,
                inactive_rounds,
                threshold,
                "Liveness violation detected"
            );
            Some(self.record_offense(node, SlashOffense::LivenessViolation))
        } else {
            tracing::debug!(
                node = ?&node[..4],
                last_active_round,
                current_round,
                inactive_rounds,
                threshold,
                "Node liveness OK"
            );
            None
        }
    }

    /// Returns `true` if the node has accumulated enough slash points to be
    /// considered *slashed* (points ≥ `slash_threshold`).
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// `true` if the node's slash points are at or above the slash threshold.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense};
    ///
    /// let mut engine = SlashingEngine::new(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 3;
    ///
    /// engine.register_validator(node, 10_000);
    /// assert!(!engine.is_slashed(&node));
    ///
    /// engine.record_offense(node, SlashOffense::Equivocation); // +500 points
    /// assert!(engine.is_slashed(&node));
    /// ```
    pub fn is_slashed(&self, node: &NodeId) -> bool {
        self.slash_points
            .get(node)
            .map(|&p| p >= self.slash_threshold)
            .unwrap_or(false)
    }

    /// Returns `true` if the node has accumulated enough slash points to be
    /// *ejected* (points ≥ `ejection_threshold`).
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// `true` if the node's slash points are at or above the ejection threshold.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense};
    ///
    /// let mut engine = SlashingEngine::new(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 9;
    ///
    /// engine.register_validator(node, 10_000);
    /// assert!(!engine.is_ejected(&node));
    ///
    /// // 4 × Equivocation = 2000 points → ejection
    /// for _ in 0..4 {
    ///     engine.record_offense(node, SlashOffense::Equivocation);
    /// }
    /// assert!(engine.is_ejected(&node));
    /// ```
    pub fn is_ejected(&self, node: &NodeId) -> bool {
        self.slash_points
            .get(node)
            .map(|&p| p >= self.ejection_threshold)
            .unwrap_or(false)
    }

    /// Returns the staked amount for a node.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// The staked amount, or `0` if the node has not been registered.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::SlashingEngine;
    ///
    /// let mut engine = SlashingEngine::new(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 1;
    ///
    /// assert_eq!(engine.stake_of(&node), 0);
    /// engine.register_validator(node, 10_000);
    /// assert_eq!(engine.stake_of(&node), 10_000);
    /// ```
    pub fn stake_of(&self, node: &NodeId) -> u64 {
        self.stakes.get(node).copied().unwrap_or(0)
    }

    /// Returns the accumulated slash points for a node.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// The total slash points, or `0` if the node has no recorded offenses.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense};
    ///
    /// let mut engine = SlashingEngine::new(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 2;
    ///
    /// assert_eq!(engine.slash_points_of(&node), 0);
    /// engine.register_validator(node, 10_000);
    /// engine.record_offense(node, SlashOffense::LivenessViolation);
    /// assert_eq!(engine.slash_points_of(&node), 100);
    /// ```
    pub fn slash_points_of(&self, node: &NodeId) -> u64 {
        self.slash_points.get(node).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    #[test]
    fn test_slash_offense_points() {
        assert_eq!(SlashOffense::Equivocation.points(), 500);
        assert_eq!(SlashOffense::LivenessViolation.points(), 100);
        assert_eq!(SlashOffense::InvalidAttestation.points(), 300);
    }

    #[test]
    fn test_default_slashing_engine() {
        let engine = SlashingEngine::default();
        let n = node(1);
        assert!(!engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n));
        assert_eq!(engine.stake_of(&n), 0);
        assert_eq!(engine.slash_points_of(&n), 0);
    }

    #[test]
    fn test_register_validator() {
        let mut engine = SlashingEngine::new(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        assert_eq!(engine.stake_of(&n), 10_000);
        assert_eq!(engine.slash_points_of(&n), 0);
    }

    #[test]
    fn test_warned_outcome() {
        let mut engine = SlashingEngine::new(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        let outcome = engine.record_offense(n, SlashOffense::LivenessViolation);
        assert_eq!(
            outcome,
            SlashOutcome::Warned {
                node: n,
                points: 100
            }
        );
    }

    #[test]
    fn test_slashed_outcome() {
        let mut engine = SlashingEngine::new(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        let outcome = engine.record_offense(n, SlashOffense::Equivocation);
        // 500 points >= 500 slash_threshold
        assert_eq!(
            outcome,
            SlashOutcome::Slashed {
                node: n,
                amount: 10_000
            }
        );
        assert!(engine.is_slashed(&n));
    }

    #[test]
    fn test_ejected_outcome() {
        let mut engine = SlashingEngine::new(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        // Accumulate 2000 points: 4 × Equivocation
        engine.record_offense(n, SlashOffense::Equivocation); // 500
        assert!(engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n));

        engine.record_offense(n, SlashOffense::Equivocation); // 1000
        assert!(!engine.is_ejected(&n));

        engine.record_offense(n, SlashOffense::Equivocation); // 1500
        assert!(!engine.is_ejected(&n));

        let outcome = engine.record_offense(n, SlashOffense::Equivocation); // 2000
        assert_eq!(outcome, SlashOutcome::Ejected { node: n });
        assert!(engine.is_ejected(&n));
    }

    #[test]
    fn test_accumulated_points_across_offenses() {
        let mut engine = SlashingEngine::new(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        engine.record_offense(n, SlashOffense::LivenessViolation); // 100
        engine.record_offense(n, SlashOffense::LivenessViolation); // 200
        engine.record_offense(n, SlashOffense::InvalidAttestation); // 500
        assert!(engine.is_slashed(&n));
        assert_eq!(engine.slash_points_of(&n), 500);
    }

    #[test]
    fn test_honest_node_never_slashed() {
        let mut engine = SlashingEngine::new(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        assert!(!engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n));
        assert_eq!(engine.slash_points_of(&n), 0);
    }

    #[test]
    fn test_liveness_check_no_violation() {
        let mut engine = SlashingEngine::new(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        let result = engine.check_liveness(n, 5, 10, 10);
        assert!(result.is_none());
    }

    #[test]
    fn test_liveness_check_violation() {
        let mut engine = SlashingEngine::new(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        let result = engine.check_liveness(n, 5, 20, 10);
        assert!(result.is_some());
        assert_eq!(engine.slash_points_of(&n), 100);
    }

    #[test]
    fn test_stake_of_unregistered() {
        let engine = SlashingEngine::new(500, 2000);
        let n = node(99);
        assert_eq!(engine.stake_of(&n), 0);
        assert_eq!(engine.slash_points_of(&n), 0);
    }

    #[test]
    fn test_check_equivocation() {
        use crate::crypto::generate_keypair;
        use crate::vector_clock::VectorClock;

        let n1 = node(1);
        let kp = generate_keypair();

        // Two events with same creator and sequence but different IDs
        let vc1 = VectorClock::with_node(n1, 1);
        let mut e1 = Event::new(n1, 0, vc1.clone(), None, None, vec![1]);
        e1.sign_with_keypair(&kp);

        let mut e2 = Event::new(n1, 0, vc1, None, None, vec![2]); // different payload → different id
        e2.sign_with_keypair(&kp);

        assert!(SlashingEngine::check_equivocation(&e1, &e2));

        // Same event → not equivocation
        assert!(!SlashingEngine::check_equivocation(&e1, &e1));
    }

    #[test]
    fn test_check_no_equivocation_different_sequence() {
        use crate::crypto::generate_keypair;
        use crate::vector_clock::VectorClock;

        let n1 = node(1);
        let kp = generate_keypair();

        let vc = VectorClock::with_node(n1, 1);
        let mut e1 = Event::new(n1, 0, vc.clone(), None, None, vec![1]);
        e1.sign_with_keypair(&kp);

        let mut e2 = Event::new(n1, 1, vc, None, None, vec![1]); // different sequence
        e2.sign_with_keypair(&kp);

        assert!(!SlashingEngine::check_equivocation(&e1, &e2));
    }
}
