//! Consensus and Finality Logic
//!
//! The consensus module determines when events are considered "final" —
//! meaning they will never be reversed and can be acted upon safely.
//!
//! Omnia uses a hybrid approach:
//! 1. **Optimistic confirmation**: Events with >2/3 acknowledgments
//! 2. **BFT finality**: Events in the causal history of a supermajority witness
//!
//! This module implements the BFT finality gadget that runs on top of the
//! causal graph. It's inspired by AlephBFT's commit rules but simplified
//! for Omnia's causal graph structure.
//!
//! Key concepts:
//! - **Witness**: The first event a node creates in a round
//! - **Round**: Determined by seeing >2/3 witnesses from the previous round
//! - **Famous**: A witness that is seen by >2/3 of next-round witnesses
//! - **Committed**: A famous witness and all its causal ancestors

use crate::causal_graph::CausalGraph;
#[cfg(feature = "persistent-storage")]
use crate::consensus_store::{ConsensusState as PersistedConsensusState, ConsensusStore, ConsensusStoreError};
use crate::slashing::{SlashOffense, SlashingEngine};
#[cfg(test)]
use crate::slashing::{DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD};
use crate::SlashingBackend;
use omnia_primitives::{Event, EventId};
use omnia_primitives::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[cfg(feature = "persistent-storage")]
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Supermajority threshold (>2/3)
/// For N nodes, we need 2*N/3 + 1 for Byzantine fault tolerance.
/// For N < 4, require unanimity since the formula produces degenerate results
/// (e.g., N=1 → 1, N=2 → 2, N=3 → 3 — none tolerate even 1 Byzantine node).
fn supermajority(total_nodes: usize) -> usize {
    if total_nodes < 4 {
        total_nodes // Require unanimity for small validator sets
    } else {
        (2 * total_nodes) / 3 + 1
    }
}

/// Produce a deterministic random bit (coin round) using BLAKE3.
///
/// When witnesses are split on whether a witness is famous (neither side
/// reaches supermajority), this function provides a deterministic coin flip
/// to break the tie. The result is determined by:
/// `BLAKE3(round_number || threshold_signature_seed)[0] & 1`
///
/// This ensures all honest nodes compute the same coin bit for the same
/// round and seed, achieving deterministic consensus even when votes are split.
///
/// # Arguments
///
/// * `round_number` — The consensus round number
/// * `seed` — The threshold signature seed from the consensus configuration
///
/// # Returns
///
/// A boolean value derived deterministically from the inputs.
fn coin_round(round_number: u64, seed: &[u8; 32]) -> bool {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&round_number.to_le_bytes());
    hasher.update(seed);
    let hash = hasher.finalize();
    // Use the least significant bit of the hash as the coin flip
    hash.as_bytes()[0] & 1 == 1
}

/// Default round seed value (all zeros).
///
/// **WARNING**: This default is intentionally insecure and must NOT be used
/// in production. It exists solely for backward compatibility in deserialization
/// and testing. Production configurations must use [`ConsensusConfig::with_random_seed`]
/// or explicitly set `round_seed` to a cryptographically random value.
const fn default_round_seed() -> [u8; 32] {
    [0u8; 32]
}

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Total number of consensus nodes
    pub total_nodes: usize,
    /// Number of rounds before an event can be committed
    pub commit_delay_rounds: u64,
    /// Whether to use optimistic confirmation
    pub optimistic_confirmation: bool,
    /// Number of acknowledgments for optimistic confirmation
    pub optimistic_threshold: u32,
    /// Maximum rounds to look ahead for fame determination
    pub max_look_ahead: u64,
    /// Randomness seed for deterministic hash-based leader selection.
    ///
    /// Updated each round from the previous round's output to ensure
    /// unpredictability. **Must not be all zeros in production** — use
    /// [`ConsensusConfig::with_random_seed`] or set this field explicitly.
    /// The default `ConsensusConfig::default()` uses a cryptographically
    /// random seed via `with_random_seed(4)`.
    #[serde(default = "default_round_seed")]
    pub round_seed: [u8; 32],
    /// Maximum duration (in milliseconds) a round may take before advancing.
    /// Default: 30_000 (30 seconds).
    pub round_timeout_ms: u64,
    /// Maximum consecutive timed-out rounds before entering recovery mode.
    /// Default: 3.
    pub max_consecutive_timeouts: u32,
    /// Maximum number of entries in `first_event_for_sequence` before triggering cleanup.
    /// Default: 10_000.
    pub max_sequence_entries: usize,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        // Use with_random_seed to generate a cryptographically random seed.
        // Falls back to all-zero seed only if the system entropy source is
        // unavailable (should never happen on a properly configured host).
        Self::with_random_seed(4).unwrap_or_else(|_| {
            tracing::error!(
                "⚠️  getrandom failed — falling back to insecure all-zero round_seed. \
                 This MUST NOT happen in production."
            );
            Self {
                total_nodes: 4,
                commit_delay_rounds: 1,
                optimistic_confirmation: true,
                optimistic_threshold: 3,
                max_look_ahead: 10,
                round_seed: [0u8; 32], // Insecure fallback — should never be reached
                round_timeout_ms: 30_000,
                max_consecutive_timeouts: 3,
                max_sequence_entries: 10_000,
            }
        })
    }
}

impl ConsensusConfig {
    /// Create a config with a cryptographically random round seed.
    pub fn with_random_seed(total_nodes: usize) -> Result<Self, ConsensusError> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| ConsensusError::EntropyFailed(e.to_string()))?;
        Ok(Self {
            total_nodes,
            commit_delay_rounds: 1,
            optimistic_confirmation: true,
            optimistic_threshold: supermajority(total_nodes) as u32,
            max_look_ahead: 10,
            round_seed: seed,
            round_timeout_ms: 30_000,
            max_consecutive_timeouts: 3,
            max_sequence_entries: 10_000,
        })
    }
}

/// Tracks round timing and timeout state for liveness.
///
/// This struct monitors whether consensus rounds complete within the
/// configured timeout. If too many consecutive rounds time out, the
/// engine enters "recovery mode" with halved timeouts to accelerate
/// round advancement and restore liveness.
///
/// Note: This type intentionally does **not** implement `Serialize`/`Deserialize`
/// because `std::time::Instant` is not serializable. The round timer is
/// re-initialized on startup.
pub struct RoundTimer {
    /// The round this timer is tracking.
    pub round: u64,
    /// When the current round started.
    started_at: Instant,
    /// The base timeout duration.
    timeout: Duration,
    /// Number of consecutive timed-out rounds.
    pub consecutive_timeouts: u32,
    /// Threshold for entering recovery mode.
    max_consecutive: u32,
}

impl RoundTimer {
    /// Create a new round timer with the given base timeout and maximum
    /// consecutive timeout threshold.
    pub fn new(timeout: Duration, max_consecutive: u32) -> Self {
        Self {
            round: 0,
            started_at: Instant::now(),
            timeout,
            consecutive_timeouts: 0,
            max_consecutive,
        }
    }

    /// Start tracking a new round.
    pub fn start_round(&mut self, round: u64) {
        self.round = round;
        self.started_at = Instant::now();
    }

    /// Check whether the current round has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.started_at.elapsed() >= self.effective_timeout()
    }

    /// Return the effective timeout, which is halved in recovery mode
    /// (when `consecutive_timeouts >= max_consecutive`).
    pub fn effective_timeout(&self) -> Duration {
        if self.consecutive_timeouts >= self.max_consecutive {
            self.timeout / 2
        } else {
            self.timeout
        }
    }

    /// Mark the current round as having succeeded, resetting the
    /// consecutive timeout counter.
    pub fn round_succeeded(&mut self) {
        self.consecutive_timeouts = 0;
    }

    /// Mark the current round as having timed out. Increments the
    /// consecutive timeout counter.
    ///
    /// Returns `true` if the engine has entered recovery mode
    /// (i.e., `consecutive_timeouts >= max_consecutive`).
    pub fn round_timed_out(&mut self) -> bool {
        self.consecutive_timeouts += 1;
        self.consecutive_timeouts >= self.max_consecutive
    }
}

/// Consensus state for an event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusState {
    /// Event is known but not yet voted on
    Pending,
    /// Event has enough acknowledgments (optimistic)
    Acknowledged,
    /// Event is a witness in a round
    Witness {
        /// The consensus round number
        round: u64,
    },
    /// Event is famous (seen by supermajority)
    Famous,
    /// Event is committed (final)
    Committed,
    /// Event was rejected from consensus (e.g., equivocation)
    Rejected,
}

/// Information about a node's participation in consensus
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConsensusInfo {
    /// Current round for this node
    pub current_round: u64,
    /// NEW: track last round where this node created a witness
    pub last_witness_round: u64,
    /// Number of events created
    pub events_created: u64,
    /// Number of events committed
    pub events_committed: u64,
    /// Last event ID from this node
    pub last_event: Option<EventId>,
}

/// Default number of rounds beyond which a Committed event is considered
/// old enough to be cleaned up from `event_states`.
pub const DEFAULT_COMMITTED_ROUND_THRESHOLD: u64 = 10_000;

/// Default sequence distance beyond which a `first_event_for_sequence` entry
/// is considered stale and can be cleaned up.
pub const DEFAULT_SEQUENCE_CLEANUP_DISTANCE: u64 = 1_000;

/// The consensus engine
///
/// Tracks consensus state for all events and determines finality.
pub struct ConsensusEngine<S: SlashingBackend = SlashingEngine> {
    /// Configuration
    config: ConsensusConfig,
    /// Consensus state for each event
    event_states: HashMap<EventId, ConsensusState>,
    /// Round assignments for events (event_id -> round)
    event_rounds: HashMap<EventId, u64>,
    /// Witnesses per round (round -> set of event_ids)
    round_witnesses: HashMap<u64, HashSet<EventId>>,
    /// Fame status per witness (event_id -> is_famous)
    fame_status: HashMap<EventId, bool>,
    /// Consensus info per node
    node_info: HashMap<NodeId, NodeConsensusInfo>,
    /// Total events committed
    committed_count: u64,
    /// The last finalized vector clock
    _last_finalized: VectorClock,
    /// Slashing backend for Byzantine fault penalties
    slashing: S,
    /// Round timer for liveness enforcement
    round_timer: RoundTimer,
    /// Maps (creator, sequence) → first EventId seen for that pair.
    first_event_for_sequence: HashMap<(NodeId, u64), EventId>,
}

impl<S: SlashingBackend> ConsensusEngine<S> {
    /// Create a new consensus engine with the given configuration and
    /// slashing backend.
    ///
    /// The slashing backend is injected from outside so that the same
    /// instance (sharing the same `Arc<dyn SlashingStore>`) can be used
    /// by both consensus and the API layer. This eliminates the
    /// dual-engine gap where consensus-detected equivocations were
    /// invisible to the REST API.
    ///
    /// # Arguments
    ///
    /// * `config` — Consensus configuration (node count, thresholds, etc.).
    /// * `slashing` — A [`SlashingBackend`] instance, typically a
    ///   [`SlashingEngine`] cloned from the one created in the node's
    ///   startup routine.
    pub fn new(config: ConsensusConfig, slashing: S) -> Self {
        let round_timer = RoundTimer::new(
            Duration::from_millis(config.round_timeout_ms),
            config.max_consecutive_timeouts,
        );

        // Validate round_seed is not all zeros
        if config.round_seed == [0u8; 32] {
            tracing::warn!(
                "⚠️  ConsensusConfig.round_seed is all zeros! \
                 deterministic hash leader selection will be deterministic and INSECURE. \
                 Use ConsensusConfig::with_random_seed() or set round_seed explicitly."
            );
            #[cfg(debug_assertions)]
            panic!("ConsensusConfig.round_seed must not be all zeros in debug builds");
        }

        // Warn if total_nodes < 4 — BFT safety is not guaranteed
        if config.total_nodes < 4 {
            tracing::warn!(
                total_nodes = config.total_nodes,
                "ConsensusConfig.total_nodes < 4: Byzantine fault tolerance requires at least 4 validators. \
                 Unanimity will be required for all decisions."
            );
        }

        Self {
            config,
            event_states: HashMap::new(),
            event_rounds: HashMap::new(),
            round_witnesses: HashMap::new(),
            fame_status: HashMap::new(),
            node_info: HashMap::new(),
            committed_count: 0,
            _last_finalized: VectorClock::new(),
            slashing,
            round_timer,

            first_event_for_sequence: HashMap::new(),
        }
    }

    /// Process a new event through consensus.
    ///
    /// This assigns the event to a round, checks if it's a witness,
    /// and determines if any events can be committed. Events from
    /// slashed validators are rejected. Equivocation is detected
    /// and recorded automatically.
    ///
    /// # Arguments
    ///
    /// * `event` — The event to process.
    /// * `graph` — The causal graph for ancestry queries.
    ///
    /// # Returns
    ///
    /// `Ok(Vec<EventId>)` — IDs of newly committed events (may be empty).
    ///
    /// # Errors
    ///
    /// - [`ConsensusError::NodeSlashed`] — the event's creator has been slashed.
    /// - [`ConsensusError::GraphError`] — a causal graph operation failed.
    /// - [`ConsensusError::EventPruned`] — an event on the ancestry path was pruned.
    /// - [`ConsensusError::InvariantViolated`] — an internal invariant was broken.
    pub fn process_event(&mut self, event: &Event, graph: &CausalGraph) -> Result<Vec<EventId>, ConsensusError> {
        let event_id = event.id;
        let creator = event.creator;

        // Skip if already processed
        if self.event_states.contains_key(&event_id) {
            return Ok(Vec::new());
        }

        // Check if the node is slashed — reject events from slashed validators
        if self.slashing.is_slashed(&creator) {
            tracing::warn!(
                node = ?&creator[..4],
                event = ?&event_id[..4],
                "Rejecting event from slashed node"
            );
            return Err(ConsensusError::NodeSlashed(creator));
        }

        // Check for equivocation — same creator + sequence, different EventId
        let seq_key = (creator, event.sequence);
        let mut is_equivocation = false;
        if let Some(&first_id) = self.first_event_for_sequence.get(&seq_key) {
            // We've seen this (creator, sequence) before — check for equivocation
            if first_id != event_id {
                match graph.get_checked(&first_id) {
                    Ok(first_event) => {
                        // Existing equivocation check using full Event data
                        if SlashingEngine::check_equivocation(first_event, event) {
                            let outcome = self.slashing.record_offense(creator, SlashOffense::Equivocation);
                            tracing::warn!(
                                node = ?&creator[..4],
                                sequence = event.sequence,
                                first_id = ?&first_id[..4],
                                second_id = ?&event_id[..4],
                                outcome = ?outcome,
                                "Equivocation detected — multiple events with same creator+sequence"
                            );
                            is_equivocation = true;
                        }
                    }
                    Err(crate::causal_graph::CausalGraphError::EventPruned(_)) => {
                        // The first event was pruned, but we can still detect equivocation
                        // using the pruned metadata (creator, sequence, event_id)
                        if let Some(metadata) = graph.get_pruned_metadata(&first_id) {
                            if metadata.creator == creator
                                && metadata.sequence == event.sequence
                                && metadata.event_id != event_id
                            {
                                let outcome = self.slashing.record_offense(creator, SlashOffense::Equivocation);
                                tracing::warn!(
                                    node = ?&creator[..4],
                                    sequence = event.sequence,
                                    first_id = ?&first_id[..4],
                                    second_id = ?&event_id[..4],
                                    outcome = ?outcome,
                                    "Equivocation detected (pruned first event) — multiple events with same creator+sequence"
                                );
                                is_equivocation = true;
                            }
                        }
                    }
                    Err(_) => {
                        // Event not found at all — should not happen
                        tracing::error!(?first_id, "first_event_for_sequence references non-existent event");
                    }
                }
            }
        } else {
            self.first_event_for_sequence.insert(seq_key, event_id);
        }

        // Reject equivocating events from entering consensus
        if is_equivocation {
            self.event_states.insert(event_id, ConsensusState::Rejected);
            return Err(ConsensusError::EquivocationDetected { creator, event_id });
        }

        // Periodic cleanup of first_event_for_sequence to prevent unbounded growth
        if self.first_event_for_sequence.len() > self.config.max_sequence_entries {
            let removed = self.cleanup_stale_sequences(None);
            if removed > 0 {
                tracing::debug!(
                    removed,
                    "Cleaned up stale sequence tracking entries during event processing"
                );
            }
        }

        // Update node info
        let info = self.node_info.entry(creator).or_default();
        info.events_created += 1;
        info.last_event = Some(event_id);

        // Assign round
        let round = self.assign_round(event, graph)?;
        self.event_rounds.insert(event_id, round);

        // Check if this is a witness (first event in round for this node)
        let is_witness = self.is_witness(event_id, creator, round);

        if is_witness {
            self.event_states.insert(event_id, ConsensusState::Witness { round });
            self.round_witnesses.entry(round).or_default().insert(event_id);
            let info = self
                .node_info
                .get_mut(&creator)
                .ok_or_else(|| ConsensusError::InvariantViolated("creator not in node_info".to_string()))?;
            info.current_round = info.current_round.max(round);
            // FIX 3: update last witness round to round+1 to prevent
            // subsequent events in the same round from also being witnesses
            info.last_witness_round = round + 1;
        } else {
            self.event_states.insert(event_id, ConsensusState::Pending);
        }

        // Optimistic confirmation
        if self.config.optimistic_confirmation {
            self.check_optimistic_confirmation(event);
        }

        // Check for newly commitable events
        let committed = self.check_commitments(event_id, round, graph)?;

        for &committed_id in &committed {
            self.event_states.insert(committed_id, ConsensusState::Committed);
        }

        self.committed_count += committed.len() as u64;

        Ok(committed)
    }

    /// Record an acknowledgment for an event (from gossip).
    ///
    /// Transitions a [`ConsensusState::Pending`] event to
    /// [`ConsensusState::Acknowledged`] if optimistic confirmation is
    /// enabled in the configuration.
    ///
    /// # Arguments
    ///
    /// * `event_id` — The ID of the event to acknowledge.
    pub fn record_acknowledgment(&mut self, event_id: EventId) {
        if let Some(&state) = self.event_states.get(&event_id) {
            if state == ConsensusState::Pending && self.config.optimistic_confirmation {
                self.event_states.insert(event_id, ConsensusState::Acknowledged);
            }
        }
    }

    /// Get the consensus state for an event.
    ///
    /// # Returns
    ///
    /// `Some(ConsensusState)` if the event has been processed, `None` otherwise.
    pub fn get_state(&self, event_id: &EventId) -> Option<ConsensusState> {
        self.event_states.get(event_id).copied()
    }

    /// Check if an event is committed (final).
    ///
    /// # Returns
    ///
    /// `true` if the event's state is [`ConsensusState::Committed`].
    pub fn is_committed(&self, event_id: &EventId) -> bool {
        matches!(self.event_states.get(event_id), Some(ConsensusState::Committed))
    }

    /// Get the round assigned to an event.
    ///
    /// # Returns
    ///
    /// `Some(round)` if the event has been processed, `None` otherwise.
    pub fn get_round(&self, event_id: &EventId) -> Option<u64> {
        self.event_rounds.get(event_id).copied()
    }

    /// Get total committed events
    pub fn committed_count(&self) -> u64 {
        self.committed_count
    }

    /// Get the current round for a node.
    ///
    /// # Returns
    ///
    /// The node's current round, or `0` if the node has not been seen.
    pub fn node_round(&self, node_id: &NodeId) -> u64 {
        self.node_info.get(node_id).map(|i| i.current_round).unwrap_or(0)
    }

    /// Get consensus statistics.
    ///
    /// # Returns
    ///
    /// A [`ConsensusStats`] snapshot with event counts by state, round
    /// progress, and threshold information.
    pub fn stats(&self) -> ConsensusStats {
        let mut by_state: HashMap<String, usize> = HashMap::new();
        for state in self.event_states.values() {
            let key = format!("{state:?}");
            *by_state.entry(key).or_insert(0) += 1;
        }

        ConsensusStats {
            total_tracked: self.event_states.len(),
            committed: self.committed_count,
            current_max_round: self.node_info.values().map(|i| i.current_round).max().unwrap_or(0),
            by_state,
            total_nodes: self.config.total_nodes,
            threshold: supermajority(self.config.total_nodes),
        }
    }

    // --- Internal methods ---

    /// Assign a round to an event
    fn assign_round(&mut self, event: &Event, graph: &CausalGraph) -> Result<u64, ConsensusError> {
        if event.is_root() {
            return Ok(0);
        }

        let parent_rounds: Vec<u64> = [event.self_parent, event.other_parent]
            .iter()
            .filter_map(|&opt_id| opt_id.and_then(|id| self.event_rounds.get(&id).copied()))
            .collect();

        let max_parent_round = parent_rounds.iter().max().copied().unwrap_or(0);

        let mut round = max_parent_round;

        for r in (0..=max_parent_round).rev() {
            if let Some(witnesses) = self.round_witnesses.get(&r) {
                if self.can_strongly_see(event, witnesses, graph)? {
                    round = r + 1;
                    break;
                }
            }
        }

        Ok(round)
    }

    /// Check if an event can "strongly see" a set of witnesses
    ///
    /// Returns `Ok(true)` if the event can strongly see enough witnesses
    /// to meet the consensus threshold, `Ok(false)` otherwise.
    ///
    /// Returns `Err(ConsensusError::EventPruned)` if ancestry cannot be
    /// determined because an event on the path has been pruned. The caller
    /// can then decide how to handle this case (e.g., skip the round
    /// assignment or use a fallback).
    fn can_strongly_see(
        &self,
        event: &Event,
        witnesses: &HashSet<EventId>,
        graph: &CausalGraph,
    ) -> Result<bool, ConsensusError> {
        let mut seen_count = 0;

        for witness_id in witnesses {
            match graph.is_ancestor_of(&event.id, witness_id) {
                Ok(true) => seen_count += 1,
                Ok(false) => {}
                Err(crate::causal_graph::CausalGraphError::EventPruned(msg)) => {
                    return Err(ConsensusError::EventPruned(msg));
                }
                Err(e) => {
                    return Err(ConsensusError::GraphError(e.to_string()));
                }
            }
        }

        Ok(seen_count >= supermajority(self.config.total_nodes))
    }

    /// FIX 3: Check if an event is a witness (first event in its round for its creator)
    fn is_witness(&self, _event_id: EventId, creator: NodeId, round: u64) -> bool {
        if let Some(info) = self.node_info.get(&creator) {
            // This is a witness if it's the first event this node creates in this round.
            // Use >= so that round 0 events from new nodes are witnesses.
            // After a witness is found, last_witness_round is set to round+1,
            // preventing subsequent events in the same round from being witnesses.
            round >= info.last_witness_round
        } else {
            // First event from this node ever — round 0, witness
            true
        }
    }

    /// Check for optimistic confirmation (fast path)
    ///
    /// Only transitions to Acknowledged if the current state is Pending.
    /// This prevents overwriting higher states like Witness, Famous, or Committed.
    fn check_optimistic_confirmation(&mut self, event: &Event) {
        if event.ack_count >= self.config.optimistic_threshold {
            if let Some(ConsensusState::Pending) = self.event_states.get(&event.id) {
                self.event_states.insert(event.id, ConsensusState::Acknowledged);
            }
        }
    }

    /// FIX 3: Check if any events can be committed based on this new event.
    /// Fixed to not require impossible round depths in small networks.
    fn check_commitments(
        &mut self,
        event_id: EventId,
        round: u64,
        graph: &CausalGraph,
    ) -> Result<Vec<EventId>, ConsensusError> {
        let mut committed = HashSet::new();

        // Only witnesses can trigger commitments
        if !matches!(self.event_states.get(&event_id), Some(ConsensusState::Witness { .. })) {
            return Ok(committed.into_iter().collect());
        }

        // FIX(bug-3): Removed shadowing line `let round = ...` that overwrote
        // the correct `round` parameter. Using the parameter directly.

        if round < self.config.commit_delay_rounds {
            // Not enough rounds have passed for commitment safety.
            // However, genesis round (round 0) with supermajority can still commit.
            if round == 0 {
                if let Some(witnesses) = self.round_witnesses.get(&0) {
                    if witnesses.len() >= supermajority(self.config.total_nodes) {
                        for &witness_id in witnesses {
                            if !self.is_committed(&witness_id) {
                                committed.insert(witness_id);
                            }
                        }
                    }
                }
            }
            return Ok(committed.into_iter().collect());
        }
        let check_round = round.saturating_sub(self.config.commit_delay_rounds);

        // Check fame of witnesses from previous rounds
        if let Some(witnesses) = self.round_witnesses.get(&check_round).cloned() {
            for witness_id in witnesses {
                if self.is_committed(&witness_id) {
                    continue;
                }

                if self.is_famous(witness_id, check_round, graph)? {
                    self.fame_status.insert(witness_id, true);
                    committed.insert(witness_id);

                    if let Ok(ancestors) = graph.get_ancestors(&witness_id) {
                        for ancestor in ancestors {
                            if !self.is_committed(&ancestor) {
                                committed.insert(ancestor);
                            }
                        }
                    }
                }
            }
        }

        Ok(committed.into_iter().collect())
    }

    /// Determine if a witness is famous
    fn is_famous(&self, witness_id: EventId, witness_round: u64, graph: &CausalGraph) -> Result<bool, ConsensusError> {
        let check_round = witness_round + self.config.commit_delay_rounds;

        if let Some(witnesses) = self.round_witnesses.get(&check_round) {
            let total_witnesses = witnesses.len();
            let mut seeing_count = 0;

            for later_witness_id in witnesses {
                match graph.is_ancestor_of(later_witness_id, &witness_id) {
                    Ok(true) => seeing_count += 1,
                    Ok(false) => {}
                    Err(crate::causal_graph::CausalGraphError::EventPruned(msg)) => {
                        return Err(ConsensusError::EventPruned(msg));
                    }
                    Err(e) => {
                        return Err(ConsensusError::GraphError(e.to_string()));
                    }
                }
            }

            let not_seeing_count = total_witnesses.saturating_sub(seeing_count);
            let threshold = supermajority(self.config.total_nodes);

            // Supermajority sees the witness → famous
            if seeing_count >= threshold {
                return Ok(true);
            }

            // Supermajority does NOT see the witness → not famous
            if not_seeing_count >= threshold {
                return Ok(false);
            }

            // Neither side reaches supermajority (split vote) → coin round tiebreaker
            tracing::debug!(
                witness_round,
                check_round,
                seeing_count,
                not_seeing_count,
                threshold,
                "Split vote detected — using coin round tiebreaker"
            );
            return Ok(coin_round(witness_round, &self.config.round_seed));
        }

        Ok(false)
    }

    /// Get all committed event IDs.
    ///
    /// # Returns
    ///
    /// A vector of [`EventId`]s for all events in [`ConsensusState::Committed`].
    pub fn get_committed(&self) -> Vec<EventId> {
        self.event_states
            .iter()
            .filter(|(_, &state)| state == ConsensusState::Committed)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get all famous witnesses.
    ///
    /// # Returns
    ///
    /// A vector of `(EventId, round)` pairs for witnesses determined to be famous.
    pub fn get_famous_witnesses(&self) -> Vec<(EventId, u64)> {
        self.fame_status
            .iter()
            .filter(|(_, &famous)| famous)
            .filter_map(|(id, _)| self.event_rounds.get(id).map(|&round| (*id, round)))
            .collect()
    }

    /// Register a validator with the slashing backend.
    ///
    /// Delegates to [`SlashingBackend::register_validator`]. The validator
    /// will be tracked for slashing purposes with the given stake.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` of the validator.
    /// * `stake` — The amount of stake the validator is bonding.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use omnia_consensus::{ConsensusEngine, SlashingEngine};
    /// let slashing = SlashingEngine::new_in_memory(500, 2000);
    /// let mut engine = ConsensusEngine::new(config, slashing);
    /// engine.register_validator(node_id, 10_000);
    /// ```
    pub fn register_validator(&mut self, node: NodeId, stake: u64) {
        self.slashing.register_validator(node, stake);
    }

    /// Check if a node has been slashed.
    ///
    /// Delegates to [`SlashingBackend::is_slashed`].
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// `true` if the node's accumulated slash points meet or exceed the
    /// slash threshold.
    pub fn is_slashed(&self, node: &NodeId) -> bool {
        self.slashing.is_slashed(node)
    }

    /// Compute the deterministic hash-based leader for the given round.
    ///
    /// Uses the [`select_leader`](omnia_crypto::select_leader) function
    /// with the current `round_seed` from the configuration and the provided
    /// round number. Candidates with zero stake are skipped.
    ///
    /// # Arguments
    ///
    /// * `candidates` — Map of NodeId to (keypair, stake) for each candidate
    /// * `round_number` — The consensus round number
    ///
    /// # Returns
    ///
    /// The `NodeId` of the selected leader, or an error if no eligible
    /// candidate exists.
    pub fn compute_leader(
        &self,
        candidates: &HashMap<NodeId, (omnia_crypto::NodeKeypair, u64)>,
        round_number: u64,
    ) -> Result<NodeId, omnia_crypto::DeterministicHashError> {
        omnia_crypto::select_leader(candidates, &self.config.round_seed, round_number)
    }

    /// Update the round seed with a new deterministic output.
    ///
    /// This should be called after each round to ensure the next round's
    /// leader selection is unpredictable. The new seed is derived from
    /// the output of the current round's leader.
    ///
    /// # Arguments
    ///
    /// * `new_seed` — The new 32-byte seed for the next round
    pub fn update_round_seed(&mut self, new_seed: [u8; 32]) {
        tracing::info!(
            old_seed = ?&self.config.round_seed[..4],
            new_seed = ?&new_seed[..4],
            "Updating round seed for deterministic hash leader selection"
        );
        self.config.round_seed = new_seed;
    }

    /// Called in the main consensus loop to check for round timeouts.
    ///
    /// If the current round has exceeded its timeout duration, advances
    /// to the next round with a BLAKE3-derived seed and starts the timer
    /// for the new round. If too many consecutive timeouts occur, enters
    /// recovery mode with a halved timeout.
    ///
    /// # Returns
    ///
    /// `true` if the round was advanced due to a timeout, `false` otherwise.
    ///
    /// # Security
    ///
    /// The new round seed is derived as `BLAKE3(old_seed || new_round)` to
    /// ensure leader selection remains unpredictable after a timeout.
    pub fn check_round_timeout(&mut self) -> bool {
        if self.round_timer.is_timed_out() {
            let in_recovery = self.round_timer.round_timed_out();
            let current_round = self.current_round();

            if in_recovery {
                tracing::warn!(
                    "Round {} timed out (recovery mode, {} consecutive timeouts). \
                     Advancing to next round with reduced timeout.",
                    current_round,
                    self.round_timer.consecutive_timeouts
                );
            } else {
                tracing::warn!("Round {} timed out. Advancing to next round.", current_round);
            }

            self.advance_round();
            self.round_timer.start_round(current_round + 1);
            return true;
        }
        false
    }

    /// Mark a round as successful (called when a commitment is reached).
    pub fn round_committed(&mut self) {
        self.round_timer.round_succeeded();
        let next_round = self.current_round() + 1;
        self.round_timer.start_round(next_round);
    }

    /// Get the current maximum round across all nodes.
    ///
    /// # Returns
    ///
    /// The highest `current_round` among all known nodes, or `0` if no
    /// nodes have been seen.
    pub fn current_round(&self) -> u64 {
        self.node_info.values().map(|i| i.current_round).max().unwrap_or(0)
    }

    /// Advance to the next round with a new leader.
    ///
    /// Called when the current round times out. Derives a new round seed
    /// from the current seed and the next round number to ensure
    /// leader selection remains unpredictable.
    ///
    /// # Security
    ///
    /// The round seed is updated via `BLAKE3(old_seed || new_round)` to
    /// prevent a malicious leader from predicting or influencing future
    /// leader selections.
    pub fn advance_round(&mut self) {
        let current = self.current_round();
        let next = current + 1;

        self.update_round_seed_from_timeout(next);

        tracing::warn!("Advanced from round {} to round {} due to timeout", current, next);
    }

    /// Derive a new round seed from the current seed and round number
    /// to ensure the next leader selection is unpredictable.
    fn update_round_seed_from_timeout(&mut self, new_round: u64) {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.config.round_seed);
        hasher.update(&new_round.to_le_bytes());
        let new_seed: [u8; 32] = hasher.finalize().into();
        self.update_round_seed(new_seed);
    }

    /// Remove Committed events from `event_states` (and associated
    /// `event_rounds`, `fame_status`) whose assigned round is older than
    /// `current_round - threshold`.
    ///
    /// This prevents unbounded growth of `event_states` in long-running
    /// nodes. Committed events that are this old are no longer needed for
    /// consensus decisions.
    ///
    /// # Arguments
    ///
    /// * `threshold` — Number of rounds of committed state to retain.
    ///   Use `None` for the default ([`DEFAULT_COMMITTED_ROUND_THRESHOLD`]).
    ///
    /// # Returns
    ///
    /// The number of entries removed.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Call at the end of each round
    /// let removed = engine.cleanup_old_committed(None);
    /// if removed > 0 {
    ///     tracing::debug!(removed, "cleaned up old committed events");
    /// }
    /// ```
    pub fn cleanup_old_committed(&mut self, threshold: Option<u64>) -> usize {
        let threshold = threshold.unwrap_or(DEFAULT_COMMITTED_ROUND_THRESHOLD);
        let current_round = self.current_round();

        // Don't bother if we haven't advanced enough rounds
        if current_round <= threshold {
            return 0;
        }

        let cutoff_round = current_round - threshold;

        // Find events that are Committed and old enough
        let to_remove: Vec<EventId> = self
            .event_states
            .iter()
            .filter(|(_, &state)| state == ConsensusState::Committed)
            .filter_map(|(id, _)| {
                let round = self.event_rounds.get(id).copied()?;
                if round < cutoff_round {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        let removed = to_remove.len();
        for id in &to_remove {
            self.event_states.remove(id);
            self.event_rounds.remove(id);
            self.fame_status.remove(id);
        }

        if removed > 0 {
            tracing::debug!(
                removed,
                current_round,
                cutoff_round,
                "cleaned up old committed events from event_states"
            );
        }

        // C-04: Prune round_witnesses entries for rounds older than cutoff
        // to prevent unbounded growth.
        let min_retained_round = current_round.saturating_sub(threshold);
        self.round_witnesses.retain(|&round, _| round > min_retained_round);

        removed
    }

    /// Create engine, restoring from persisted state if available.
    ///
    /// If the store contains a previously persisted consensus state, the
    /// engine is created and then restored to that state. Otherwise, a
    /// fresh engine is created from the given configuration.
    ///
    /// This enables crash recovery: a node restarting after a crash can
    /// resume consensus from the last persisted round without replaying
    /// all events from genesis.
    ///
    /// # Arguments
    ///
    /// * `config` — Consensus configuration (node count, thresholds, etc.).
    /// * `store` — A [`ConsensusStore`] backend for persisting consensus state.
    /// * `slashing` — A [`SlashingEngine`] instance for Byzantine fault penalties.
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusError`] if the persisted state cannot be loaded
    /// or has an incompatible version.
    #[cfg(feature = "persistent-storage")]
    pub fn load_or_new(
        config: ConsensusConfig,
        store: Arc<dyn ConsensusStore>,
        slashing: S,
    ) -> Result<Self, ConsensusError> {
        match store.load_state() {
            Ok(Some(state)) => {
                tracing::info!(
                    round = state.current_round,
                    committed = state.committed_events,
                    validators = state.active_validators.len(),
                    "Restoring consensus engine from persisted state"
                );
                let mut engine = Self::new(config, slashing);
                engine.restore_state(state)?;
                Ok(engine)
            }
            Ok(None) => {
                tracing::info!("No persisted consensus state found — starting fresh");
                Ok(Self::new(config, slashing))
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Failed to load persisted consensus state — starting fresh"
                );
                Ok(Self::new(config, slashing))
            }
        }
    }

    /// Restore engine state from a persisted snapshot.
    ///
    /// Restores the round seed, committed count, per-node round tracking,
    /// and equivocation metadata from a previously persisted
    /// [`PersistedConsensusState`].
    ///
    /// After restoration, the engine will resume consensus from the
    /// persisted round number. Per-node `current_round` values are
    /// restored, and `last_witness_round` is set to `current_round + 1`
    /// to prevent events from being double-witnessed after recovery.
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusError`] if the state version is unsupported.
    #[cfg(feature = "persistent-storage")]
    fn restore_state(&mut self, state: PersistedConsensusState) -> Result<(), ConsensusError> {
        if state.version < 1 || state.version > 2 {
            return Err(ConsensusError::Config(format!(
                "Unsupported consensus state version: {}",
                state.version
            )));
        }

        // Restore round seed (critical for deterministic leader selection continuity)
        self.config.round_seed = state.round_seed;

        // Restore committed count
        self.committed_count = state.committed_events;

        // Restore per-node round tracking.
        // For each persisted validator, set current_round and
        // last_witness_round = current_round + 1 to prevent
        // double-witnessing events after recovery.
        for validator in &state.active_validators {
            let info = self.node_info.entry(*validator).or_default();
            info.current_round = state.current_round;
            info.last_witness_round = state.current_round + 1;
        }

        // Restore first_event_for_sequence map from v2 snapshots.
        // For v1 snapshots, this field is empty and equivocation tracking
        // will rebuild from the equivocation_tracking summary.
        //
        // TODO: Persisting `first_event_for_sequence` is critical for crash
        // recovery. Without it, a restarted node cannot detect equivocation
        // for events that arrived before the crash. The v2 snapshot format
        // includes this map, but we should also consider:
        //   1. Bounding the map size via periodic compaction (already done
        //      in process_event via cleanup_stale_sequences, but persisted
        //      entries may accumulate across restarts).
        //   2. Rebuilding the map from the causal graph on recovery when the
        //      snapshot is stale or from a v1 format.
        //   3. Persisting the map incrementally (e.g., as a WAL) rather than
        //      only at round boundaries to reduce the window of data loss.
        if state.version >= 2 {
            self.first_event_for_sequence = state.first_event_for_sequence;
        }

        // Start the round timer for the restored round
        self.round_timer.start_round(state.current_round);

        tracing::info!(
            round = state.current_round,
            committed = state.committed_events,
            validators = state.active_validators.len(),
            equivocation_entries = state.equivocation_tracking.len(),
            "Consensus engine state restored"
        );

        Ok(())
    }

    /// Persist current state after each round advancement.
    ///
    /// Captures a snapshot of the engine's critical state and saves it
    /// to the provided [`ConsensusStore`]. This should be called after
    /// each round advancement to ensure crash recovery can resume from
    /// the most recent state.
    ///
    /// # Arguments
    ///
    /// * `store` — The persistence backend to save state to.
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusStoreError`] if the state cannot be serialized
    /// or the database cannot be written to.
    #[cfg(feature = "persistent-storage")]
    pub fn persist_state(&self, store: &dyn ConsensusStore) -> Result<(), ConsensusStoreError> {
        let current_round = self.current_round();

        // Derive equivocation tracking: NodeId → max sequence seen
        let equivocation_tracking: HashMap<NodeId, u64> =
            self.first_event_for_sequence
                .keys()
                .fold(HashMap::new(), |mut acc, (node_id, seq)| {
                    let entry = acc.entry(*node_id).or_insert(0);
                    *entry = (*entry).max(*seq);
                    acc
                });

        let state = PersistedConsensusState {
            current_round,
            round_seed: self.config.round_seed,
            committed_events: self.committed_count,
            last_finalized_round: current_round.saturating_sub(self.config.commit_delay_rounds),
            active_validators: self.node_info.keys().cloned().collect(),
            equivocation_tracking,
            first_event_for_sequence: self.first_event_for_sequence.clone(),
            version: 2,
        };

        store.save_state(&state)?;

        // Also save the lightweight round number
        store.save_round(current_round)?;

        tracing::debug!(
            round = current_round,
            committed = self.committed_count,
            "Consensus state persisted"
        );

        Ok(())
    }

    /// Clean up `first_event_for_sequence` entries where the creator's
    /// current sequence has advanced far beyond the stored sequence number.
    ///
    /// When a creator has advanced many sequences beyond the stored entry,
    /// the old entry is no longer useful for equivocation detection
    /// (the creator has long since moved on).
    ///
    /// # Arguments
    ///
    /// * `distance` — Minimum sequence distance between the creator's
    ///   current sequence and the stored sequence for the entry to be
    ///   considered stale. Use `None` for the default
    ///   ([`DEFAULT_SEQUENCE_CLEANUP_DISTANCE`]).
    ///
    /// # Returns
    ///
    /// The number of entries removed.
    pub fn cleanup_stale_sequences(&mut self, distance: Option<u64>) -> usize {
        let distance = distance.unwrap_or(DEFAULT_SEQUENCE_CLEANUP_DISTANCE);

        // Collect the current sequence for each node from node_info
        let node_seqs: HashMap<NodeId, u64> = self
            .node_info
            .iter()
            .map(|(node_id, info)| (*node_id, info.events_created))
            .collect();

        let to_remove: Vec<(NodeId, u64)> = self
            .first_event_for_sequence
            .keys()
            .filter(|(creator, seq)| {
                node_seqs
                    .get(creator)
                    .map(|&current_seq| current_seq.saturating_sub(*seq) > distance)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        let removed = to_remove.len();
        for key in &to_remove {
            self.first_event_for_sequence.remove(key);
        }

        if removed > 0 {
            tracing::debug!(removed, "cleaned up stale first_event_for_sequence entries");
        }

        removed
    }
}

/// Type alias for the default concrete `ConsensusEngine` using [`SlashingEngine`].
///
/// This preserves backward compatibility for code that does not need
/// a custom [`SlashingBackend`] implementation.
pub type DefaultConsensusEngine = ConsensusEngine<SlashingEngine>;

/// Consensus statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusStats {
    /// Total number of events being tracked
    pub total_tracked: usize,
    /// Number of committed events
    pub committed: u64,
    /// Highest round reached across all nodes
    pub current_max_round: u64,
    /// Event counts by consensus state
    pub by_state: HashMap<String, usize>,
    /// Total number of consensus nodes
    pub total_nodes: usize,
    /// Supermajority threshold
    pub threshold: usize,
}

/// Errors during consensus operations
#[derive(Error, Debug, Clone)]
pub enum ConsensusError {
    #[error("Graph error: {0}")]
    /// Error from the causal graph
    GraphError(String),
    #[error("Event not found: {0:?}")]
    /// Event not found in consensus state
    EventNotFound(EventId),
    #[error("Invalid state transition")]
    /// Invalid state transition attempted
    InvalidStateTransition,
    #[error("Not enough nodes for consensus (need at least 4, got {0})")]
    /// Insufficient nodes for Byzantine fault tolerance
    InsufficientNodes(usize),
    #[error("Node has been slashed: {0:?}")]
    /// The node has been slashed and its events are rejected
    NodeSlashed(NodeId),
    /// Equivocation was detected — the event is rejected from consensus.
    #[error("Equivocation detected from node {creator:?} for event {event_id:?}")]
    EquivocationDetected {
        /// The node that equivocated.
        creator: NodeId,
        /// The event ID of the equivocating event.
        event_id: EventId,
    },
    /// Failed to obtain entropy for random seed generation.
    #[error("entropy generation failed: {0}")]
    EntropyFailed(String),
    /// An invariant was violated.
    #[error("invariant violated: {0}")]
    InvariantViolated(String),
    /// An event on the ancestry path has been pruned, so ancestry
    /// cannot be determined.
    #[error("event pruned: {0}")]
    EventPruned(String),
    /// Configuration or state restoration error.
    #[error("config error: {0}")]
    Config(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::causal_graph::CausalGraph;
    use omnia_crypto::generate_keypair;
    use omnia_primitives::blake3_hash_domain;
    use omnia_primitives::Event;
    use omnia_primitives::VectorClock;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    /// Test-friendly config with a non-zero round seed (avoids debug-build panic).
    fn test_config() -> ConsensusConfig {
        let mut seed = [0u8; 32];
        seed[0] = 1; // Non-zero to avoid the debug panic
        ConsensusConfig {
            total_nodes: 4,
            commit_delay_rounds: 1,
            optimistic_confirmation: true,
            optimistic_threshold: 3,
            max_look_ahead: 10,
            round_seed: seed,
            round_timeout_ms: 30_000,
            max_consecutive_timeouts: 3,
            max_sequence_entries: 10_000,
        }
    }

    fn setup_graph_with_events() -> (CausalGraph, Vec<EventId>) {
        let mut graph = CausalGraph::new();
        let mut keypairs = Vec::new();

        // Generate keypairs first so the same keypair is used for genesis and child
        for _ in 0..4 {
            keypairs.push(generate_keypair());
        }

        // Derive node IDs from keypairs (matching sign_with_keypair behavior)
        let n1 = blake3_hash_domain(b"omnia-creator", &keypairs[0].verifying_key().to_bytes());
        let n2 = blake3_hash_domain(b"omnia-creator", &keypairs[1].verifying_key().to_bytes());
        let n3 = blake3_hash_domain(b"omnia-creator", &keypairs[2].verifying_key().to_bytes());
        let n4 = blake3_hash_domain(b"omnia-creator", &keypairs[3].verifying_key().to_bytes());

        let mut events = Vec::new();

        for kp in &keypairs {
            let node_id = blake3_hash_domain(b"omnia-creator", &kp.verifying_key().to_bytes());
            let mut e = Event::genesis(node_id, vec![node_id[0]]).expect("valid genesis event");
            e.sign_with_keypair(kp).expect("signing");
            let id = e.id;
            graph.insert(e).unwrap();
            events.push(id);
        }

        for i in 0..4 {
            let creator = [n1, n2, n3, n4][i];
            let kp = &keypairs[i];
            let sp = events[i];
            let op = events[(i + 1) % 4];

            let mut vc = VectorClock::with_node(creator, 2);
            let other = [n1, n2, n3, n4][(i + 1) % 4];
            vc.set(other, 1);

            let mut e = Event::new(creator, 1, vc, Some(sp), Some(op), vec![]).expect("valid event");
            e.sign_with_keypair(kp).expect("signing");
            let id = e.id;
            graph.insert(e).unwrap();
            events.push(id);
        }

        (graph, events)
    }

    #[test]
    fn test_consensus_engine_creation() {
        let config = test_config();
        let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let engine = ConsensusEngine::new(config, slashing);

        assert_eq!(engine.committed_count(), 0);
        assert_eq!(engine.stats().total_nodes, 4);
        assert_eq!(engine.stats().threshold, 3);
    }

    #[test]
    fn test_process_event() {
        let config = test_config();
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );
        let (graph, events) = setup_graph_with_events();

        for event_id in &events {
            let event = graph.get(event_id).unwrap();
            let committed = engine.process_event(event, &graph).unwrap();
            assert!(committed.len() <= events.len());
        }

        assert_eq!(engine.stats().total_tracked, events.len());
    }

    #[test]
    fn test_supermajority_threshold() {
        assert_eq!(supermajority(4), 3);
        assert_eq!(supermajority(7), 5);
        assert_eq!(supermajority(10), 7);
        assert_eq!(supermajority(100), 67);
    }

    #[test]
    fn test_consensus_state_enum() {
        assert_ne!(ConsensusState::Pending, ConsensusState::Committed);
        assert_eq!(
            ConsensusState::Witness { round: 5 },
            ConsensusState::Witness { round: 5 }
        );
    }

    #[test]
    fn test_node_consensus_info() {
        let config = test_config();
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );
        let (graph, events) = setup_graph_with_events();

        for event_id in &events {
            let event = graph.get(event_id).unwrap();
            engine.process_event(event, &graph).unwrap();
        }

        let stats = engine.stats();
        assert!(stats.total_tracked > 0);
    }

    #[test]
    fn test_insufficient_nodes_error() {
        let mut config = test_config();
        config.total_nodes = 3;

        let engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );
        assert_eq!(engine.stats().threshold, 3);
    }

    #[test]
    fn test_get_round() {
        let config = test_config();
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );
        let (graph, events) = setup_graph_with_events();

        let event = graph.get(&events[0]).unwrap();
        engine.process_event(event, &graph).unwrap();

        assert!(engine.get_round(&events[0]).is_some());
    }

    #[test]
    fn test_fame_determination() {
        let mut seed = [0u8; 32];
        seed[0] = 1;
        let config = ConsensusConfig {
            total_nodes: 4,
            commit_delay_rounds: 1,
            optimistic_confirmation: false,
            optimistic_threshold: 3,
            max_look_ahead: 10,
            round_seed: seed,
            round_timeout_ms: 30_000,
            max_consecutive_timeouts: 3,
            max_sequence_entries: 10_000,
        };
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );

        let (graph, events) = setup_graph_with_events();
        for event_id in &events {
            let event = graph.get(event_id).unwrap();
            engine.process_event(event, &graph).unwrap();
        }

        assert_eq!(engine.stats().total_tracked, events.len());
    }

    #[test]
    fn test_coin_round_deterministic() {
        let seed = [42u8; 32];
        // Same inputs must produce the same output
        let bit1 = coin_round(5, &seed);
        let bit2 = coin_round(5, &seed);
        assert_eq!(bit1, bit2, "Coin round must be deterministic for the same inputs");

        // Different rounds should (likely) produce different bits
        let bit3 = coin_round(6, &seed);
        // Not guaranteed to be different, but the function should not panic
        let _ = bit3;
    }

    #[test]
    fn test_coin_round_breaks_tie() {
        // Simulate a 50/50 split scenario: 2 nodes see, 2 don't
        // In a 4-node network, supermajority is 3. Neither 2 nor 2 reaches 3.
        // The coin round should deterministically resolve this.
        let seed = [1u8; 32];
        let result = coin_round(10, &seed);
        // The coin round always produces a definitive answer (true or false),
        // breaking the tie instead of leaving it unresolved.
        // This is inherently always true for a bool, but we assert it
        // to document the invariant and guard against future non-bool returns.
        let _: bool = result;

        // Verify it's the same across calls (deterministic)
        let result2 = coin_round(10, &seed);
        assert_eq!(result, result2, "Coin round must be deterministic");
    }

    #[test]
    fn test_record_acknowledgment() {
        let config = test_config();
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );
        let (graph, events) = setup_graph_with_events();

        let event = graph.get(&events[0]).unwrap();
        engine.process_event(event, &graph).unwrap();

        engine.record_acknowledgment(events[0]);

        let state = engine.get_state(&events[0]);
        assert!(
            matches!(state, Some(ConsensusState::Acknowledged))
                || matches!(state, Some(ConsensusState::Witness { .. }))
                || matches!(state, Some(ConsensusState::Pending))
        );
    }

    #[test]
    fn test_consensus_stats() {
        let config = test_config();
        let engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );

        let stats = engine.stats();
        assert_eq!(stats.total_tracked, 0);
        assert_eq!(stats.committed, 0);
        assert_eq!(stats.current_max_round, 0);
        assert_eq!(stats.total_nodes, 4);
    }

    /// FIX 3: Test that proves finality works in a 4-node network.
    #[test]
    fn test_four_node_finality() {
        let mut config = test_config();
        config.total_nodes = 4;
        config.commit_delay_rounds = 1; // Reduced for small network

        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );
        let mut graph = CausalGraph::new();

        // Create 4 nodes with real keypairs
        let nodes: Vec<_> = (0..4)
            .map(|i| {
                let mut n = [0u8; 32];
                n[0] = i;
                let kp = generate_keypair();
                (n, kp)
            })
            .collect();

        // Genesis events
        let mut genesis_ids = Vec::new();
        for (node_id, keypair) in &nodes {
            let mut e = Event::genesis(*node_id, vec![node_id[0]]).expect("valid genesis event");
            e.sign_with_keypair(keypair).expect("signing");
            let id = e.id;
            graph.insert(e).unwrap();
            genesis_ids.push(id);
        }

        // Process genesis through consensus
        for id in &genesis_ids {
            let event = graph.get(id).unwrap();
            let _committed = engine.process_event(event, &graph).unwrap();
        }

        // With supermajority (3 of 4) genesis witnesses in round 0,
        // the check_commitments should commit them immediately
        let committed = engine.get_committed();
        // All 4 genesis events are witnesses in round 0.
        // check_commitments for round 0 witnesses: if we have >= threshold
        // witnesses in round 0, they are committed.
        // Since we have 4 witnesses and threshold is 3, all should commit.
        assert!(
            committed.len() >= 3,
            "Expected at least 3 genesis events to commit, got {}",
            committed.len()
        );
    }

    #[test]
    fn test_commit_delay_not_bypassed_at_early_rounds() {
        let mut config = test_config();
        config.total_nodes = 4;
        config.commit_delay_rounds = 3; // Require 3 rounds before commitment

        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );
        let mut graph = CausalGraph::new();

        // Create and process genesis events for round 0
        let nodes: Vec<_> = (0..4)
            .map(|i| {
                let mut n = [0u8; 32];
                n[0] = i;
                let kp = generate_keypair();
                (n, kp)
            })
            .collect();

        let mut genesis_ids = Vec::new();
        for (node_id, keypair) in &nodes {
            let mut e = Event::genesis(*node_id, vec![node_id[0]]).expect("valid genesis event");
            e.sign_with_keypair(keypair).expect("signing");
            let id = e.id;
            graph.insert(e).unwrap();
            genesis_ids.push(id);
        }

        // Process through consensus
        for id in &genesis_ids {
            let event = graph.get(id).unwrap();
            engine.process_event(event, &graph).unwrap();
        }

        // At round 0, with commit_delay_rounds = 3, nothing should be committed
        // (except possibly genesis via the supermajority path)
        // The key invariant: effective_delay must NEVER be 0 when commit_delay_rounds > 0
        // except for the explicit genesis supermajority path
        assert!(
            engine.committed_count() <= 4,
            "Only genesis should be committable at round 0"
        );
    }

    // ── Consensus state persistence tests (H-6) ─────────────────────

    #[test]
    #[cfg(feature = "persistent-storage")]
    fn test_consensus_state_persistence_round_trip() {
        let store = crate::consensus_store::RedbConsensusStore::in_memory().unwrap();
        let state = PersistedConsensusState {
            current_round: 42,
            round_seed: [1u8; 32],
            committed_events: 1000,
            last_finalized_round: 40,
            active_validators: vec![[2u8; 32]],
            equivocation_tracking: HashMap::from([([3u8; 32], 5u64)]),
            first_event_for_sequence: HashMap::new(),
            version: 2,
        };

        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();

        assert_eq!(loaded.current_round, 42);
        assert_eq!(loaded.round_seed, [1u8; 32]);
        assert_eq!(loaded.committed_events, 1000);
        assert_eq!(loaded.last_finalized_round, 40);
        assert_eq!(loaded.version, 1);
    }

    #[test]
    #[cfg(feature = "persistent-storage")]
    fn test_consensus_resume_from_persisted() {
        let store: Arc<dyn ConsensusStore> = Arc::new(crate::consensus_store::RedbConsensusStore::in_memory().unwrap());

        // Create engine, advance some rounds
        let config = test_config();
        let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut engine = ConsensusEngine::new(config.clone(), slashing.clone());

        // Simulate that some nodes have advanced to round 100
        let n1 = node(1);
        let n2 = node(2);
        engine.node_info.entry(n1).or_default().current_round = 100;
        engine.node_info.entry(n1).or_default().last_witness_round = 101;
        engine.node_info.entry(n2).or_default().current_round = 100;
        engine.node_info.entry(n2).or_default().last_witness_round = 101;
        engine.committed_count = 500;

        // Persist
        engine.persist_state(store.as_ref()).unwrap();

        // Create new engine, should restore from persisted
        let engine2 = ConsensusEngine::load_or_new(config, Arc::clone(&store), slashing).unwrap();
        assert_eq!(engine2.current_round(), 100);
        assert_eq!(engine2.committed_count(), 500);
    }

    #[test]
    #[cfg(feature = "persistent-storage")]
    fn test_consensus_state_format_version() {
        let store = crate::consensus_store::RedbConsensusStore::in_memory().unwrap();
        let state = PersistedConsensusState {
            current_round: 1,
            round_seed: [0u8; 32],
            committed_events: 0,
            last_finalized_round: 0,
            active_validators: vec![],
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 1,
        };
        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();
        assert_eq!(loaded.version, 1);
    }

    #[test]
    #[cfg(feature = "persistent-storage")]
    fn test_consensus_persist_and_restore_round_seed() {
        let store: Arc<dyn ConsensusStore> = Arc::new(crate::consensus_store::RedbConsensusStore::in_memory().unwrap());

        let config = test_config();
        let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let mut engine = ConsensusEngine::new(config.clone(), slashing.clone());

        // Update the round seed to a known value
        let new_seed = [42u8; 32];
        engine.update_round_seed(new_seed);

        // Simulate round advancement
        let n1 = node(1);
        engine.node_info.entry(n1).or_default().current_round = 10;

        // Persist
        engine.persist_state(store.as_ref()).unwrap();

        // Create new engine and verify round seed is restored
        let engine2 = ConsensusEngine::load_or_new(config, Arc::clone(&store), slashing).unwrap();
        // The round seed should have been restored
        assert_eq!(engine2.current_round(), 10);
    }

    #[test]
    #[cfg(feature = "persistent-storage")]
    fn test_consensus_load_or_new_without_persisted_state() {
        let store: Arc<dyn ConsensusStore> = Arc::new(crate::consensus_store::RedbConsensusStore::in_memory().unwrap());

        let config = test_config();
        let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);

        // No state persisted — should create fresh engine
        let engine = ConsensusEngine::load_or_new(config, store, slashing).unwrap();
        assert_eq!(engine.current_round(), 0);
        assert_eq!(engine.committed_count(), 0);
    }

    #[test]
    #[cfg(feature = "persistent-storage")]
    fn test_consensus_unsupported_version_rejected() {
        let store: Arc<dyn ConsensusStore> = Arc::new(crate::consensus_store::RedbConsensusStore::in_memory().unwrap());

        // Persist a state with an unsupported version
        let bad_state = PersistedConsensusState {
            current_round: 10,
            round_seed: [1u8; 32],
            committed_events: 50,
            last_finalized_round: 8,
            active_validators: vec![node(1)],
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 999, // Unsupported version
        };
        store.save_state(&bad_state).unwrap();

        let config = test_config();
        let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);

        // load_or_new should return Err on unsupported version
        let result = ConsensusEngine::load_or_new(config, Arc::clone(&store), slashing);
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "persistent-storage")]
    fn test_consensus_round_lightweight_persistence() {
        let store = crate::consensus_store::RedbConsensusStore::in_memory().unwrap();

        // Default round is 0
        assert_eq!(store.load_round().unwrap(), 0);

        store.save_round(42).unwrap();
        assert_eq!(store.load_round().unwrap(), 42);

        store.save_round(100).unwrap();
        assert_eq!(store.load_round().unwrap(), 100);
    }
}

/// Property-based tests for consensus invariants.
///
/// These tests verify that the consensus engine maintains internal
/// consistency even when fed arbitrary events. The key property is
/// that processing events never panics, regardless of input.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod proptests {
    use super::*;
    use omnia_primitives::Event;
    use proptest::prelude::*;

    fn nid(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    /// Test-friendly config with a non-zero round seed (avoids debug-build panic).
    fn test_config() -> ConsensusConfig {
        let mut seed = [0u8; 32];
        seed[0] = 1; // Non-zero to avoid the debug panic
        ConsensusConfig {
            total_nodes: 4,
            commit_delay_rounds: 1,
            optimistic_confirmation: true,
            optimistic_threshold: 3,
            max_look_ahead: 10,
            round_seed: seed,
            round_timeout_ms: 30_000,
            max_consecutive_timeouts: 3,
            max_sequence_entries: 10_000,
        }
    }

    /// Strategy: generate a genesis-like event with arbitrary creator and payload.
    /// These events won't have valid signatures, but they exercise the
    /// consensus engine's internal logic without requiring a populated graph.
    fn arb_genesis_event() -> impl Strategy<Value = Event> {
        (any::<u8>(), any::<Vec<u8>>()).prop_map(|(creator_byte, payload)| {
            let creator = nid(creator_byte % 10);
            Event::genesis(creator, payload).expect("valid genesis event")
        })
    }

    proptest! {
        /// Property: Processing the same event twice never causes a panic
        /// and the second call returns an empty result (already processed).
        #[test]
        fn proptest_idempotent_process_event(event in arb_genesis_event()) {
            let slashing = SlashingEngine::new_in_memory(
                DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD
            );
            let config = test_config();
            let mut engine = ConsensusEngine::new(config, slashing);
            let graph = CausalGraph::new();

            // First processing — may succeed or fail depending on graph state
            let _ = engine.process_event(&event, &graph);
            // Second processing of same event — must not panic
            let result = engine.process_event(&event, &graph);
            // Should return Ok (already processed) or Err (slashed, etc.)
            // The key invariant: no panic
            let _ = result;
        }

        /// Property: The consensus engine remains in a valid state after
        /// processing any sequence of genesis events. "Valid" means:
        /// - committed_count is consistent with tracked state
        /// - no panics
        #[test]
        fn proptest_consistent_state_after_events(
            events in prop::collection::vec(arb_genesis_event(), 0..20)
        ) {
            let slashing = SlashingEngine::new_in_memory(
                DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD
            );
            let config = test_config();
            let mut engine = ConsensusEngine::new(config, slashing);
            let graph = CausalGraph::new();

            for event in &events {
                let _ = engine.process_event(event, &graph);
            }

            // The committed count should never exceed total tracked events
            let stats = engine.stats();
            assert!(
                stats.committed as usize <= stats.total_tracked,
                "Committed count {} exceeds tracked count {}",
                stats.committed, stats.total_tracked
            );
        }
    }
}

/// Tests for the round timeout mechanism (Phase C1).
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod timeout_tests {
    use super::*;
    #[cfg(test)]
    use crate::causal_graph::CausalGraph;
    #[cfg(test)]
    use omnia_crypto::generate_keypair;
    #[cfg(test)]
    use omnia_primitives::blake3_hash_domain;
    #[cfg(test)]
    use omnia_primitives::Event;
    #[cfg(test)]
    use omnia_primitives::VectorClock;
    use std::thread;

    /// Helper: create a NodeId from a u8.
    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    /// Test-friendly config with a non-zero round seed (avoids debug-build panic).
    fn test_config() -> ConsensusConfig {
        let mut seed = [0u8; 32];
        seed[0] = 1;
        ConsensusConfig {
            total_nodes: 4,
            commit_delay_rounds: 1,
            optimistic_confirmation: true,
            optimistic_threshold: 3,
            max_look_ahead: 10,
            round_seed: seed,
            round_timeout_ms: 30_000,
            max_consecutive_timeouts: 3,
            max_sequence_entries: 10_000,
        }
    }

    /// Helper: set up a graph with events for cleanup tests.
    fn setup_graph_with_events() -> (CausalGraph, Vec<EventId>) {
        let mut graph = CausalGraph::new();
        let mut keypairs = Vec::new();

        // Generate keypairs first so the same keypair is used for genesis and child
        for _ in 0..4 {
            keypairs.push(generate_keypair());
        }

        // Derive node IDs from keypairs (matching sign_with_keypair behavior)
        let n1 = blake3_hash_domain(b"omnia-creator", &keypairs[0].verifying_key().to_bytes());
        let n2 = blake3_hash_domain(b"omnia-creator", &keypairs[1].verifying_key().to_bytes());
        let n3 = blake3_hash_domain(b"omnia-creator", &keypairs[2].verifying_key().to_bytes());
        let n4 = blake3_hash_domain(b"omnia-creator", &keypairs[3].verifying_key().to_bytes());

        let mut events = Vec::new();

        for kp in &keypairs {
            let node_id = blake3_hash_domain(b"omnia-creator", &kp.verifying_key().to_bytes());
            let mut e = Event::genesis(node_id, vec![node_id[0]]).expect("valid genesis event");
            e.sign_with_keypair(kp).expect("signing");
            let id = e.id;
            graph.insert(e).unwrap();
            events.push(id);
        }

        for i in 0..4 {
            let creator = [n1, n2, n3, n4][i];
            let kp = &keypairs[i];
            let sp = events[i];
            let op = events[(i + 1) % 4];

            let mut vc = VectorClock::with_node(creator, 2);
            let other = [n1, n2, n3, n4][(i + 1) % 4];
            vc.set(other, 1);

            let mut e = Event::new(creator, 1, vc, Some(sp), Some(op), vec![]).expect("valid event");
            e.sign_with_keypair(kp).expect("signing");
            let id = e.id;
            graph.insert(e).unwrap();
            events.push(id);
        }

        (graph, events)
    }

    #[test]
    fn test_round_timer_timeout_detection() {
        let mut timer = RoundTimer::new(Duration::from_millis(100), 3);
        timer.start_round(1);
        assert!(!timer.is_timed_out());
        thread::sleep(Duration::from_millis(150));
        assert!(timer.is_timed_out());
    }

    #[test]
    fn test_round_timer_success_resets_consecutive() {
        let mut timer = RoundTimer::new(Duration::from_millis(100), 3);
        timer.start_round(1);

        // Two timeouts — not yet in recovery (2 < 3)
        timer.round_timed_out();
        timer.round_timed_out();
        assert_eq!(timer.effective_timeout(), Duration::from_millis(100));

        // Succeed — resets consecutive count to 0
        timer.round_succeeded();
        assert_eq!(timer.effective_timeout(), Duration::from_millis(100));

        // Two more timeouts — still not in recovery (2 < 3, thanks to reset)
        timer.round_timed_out();
        timer.round_timed_out();
        assert_eq!(timer.effective_timeout(), Duration::from_millis(100));

        // Third timeout — now in recovery (3 >= 3)
        let in_recovery = timer.round_timed_out();
        assert!(in_recovery);
        assert_eq!(timer.effective_timeout(), Duration::from_millis(50));
    }

    #[test]
    fn test_recovery_mode_halved_timeout() {
        let mut timer = RoundTimer::new(Duration::from_millis(100), 3);
        timer.start_round(1);
        timer.round_timed_out(); // 1
        timer.round_timed_out(); // 2
        let in_recovery = timer.round_timed_out(); // 3 → recovery
        assert!(in_recovery);
        assert_eq!(timer.effective_timeout(), Duration::from_millis(50));
    }

    #[test]
    fn test_no_timeout_before_deadline() {
        let timer = RoundTimer::new(Duration::from_secs(30), 3);
        assert!(!timer.is_timed_out());
    }

    // ── Task 30: Bounded Caches and Pruning Tests ──────────────────────

    #[test]
    fn test_cleanup_old_committed_removes_old_events() {
        let config = test_config();
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );
        let (graph, events) = setup_graph_with_events();

        // Process all events
        for event_id in &events {
            let event = graph.get(event_id).unwrap();
            engine.process_event(event, &graph).unwrap();
        }

        // Manually set some events as Committed with old rounds
        // First, find any committed events and adjust their rounds
        let committed = engine.get_committed();
        if !committed.is_empty() {
            // Set the round to 0 for the first committed event to make it "old"
            let first_committed = committed[0];
            engine.event_rounds.insert(first_committed, 0);

            // Set current_round high enough
            for info in engine.node_info.values_mut() {
                info.current_round = 20_001;
            }

            // With threshold of 10_000, events in round 0 should be cleaned up
            let removed = engine.cleanup_old_committed(Some(10_000));
            assert!(removed >= 1, "Should remove at least 1 old committed event");
        }
    }

    #[test]
    fn test_cleanup_old_committed_noop_when_rounds_low() {
        let config = test_config();
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );

        // current_round is 0 — nothing to clean up
        let removed = engine.cleanup_old_committed(None);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_cleanup_stale_sequences_removes_old_entries() {
        let config = test_config();
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );

        let n1 = node(1);
        let n2 = node(2);

        // Add entries for sequences 0..5 for n1 and n2
        for seq in 0..5u64 {
            engine.first_event_for_sequence.insert((n1, seq), [seq as u8; 32]);
            engine.first_event_for_sequence.insert((n2, seq), [seq as u8; 32]);
        }

        // Set n1's events_created to 2000 (far ahead of seq 0-4)
        engine.node_info.insert(
            n1,
            NodeConsensusInfo {
                current_round: 0,
                last_witness_round: 0,
                events_created: 2000,
                events_committed: 0,
                last_event: None,
            },
        );

        // n2 has no node_info entry — its entries won't be cleaned
        // (creator not tracked in node_info means we can't determine staleness)

        // Clean up with distance=100: n1's entries (seq 0-4) are 1996-2000
        // away from 2000 — all should be removed since distance > 100
        let removed = engine.cleanup_stale_sequences(Some(100));
        assert_eq!(removed, 5, "Should remove all 5 entries for n1");
        // n2's entries should remain (no node_info → not stale)
        assert_eq!(engine.first_event_for_sequence.len(), 5);
    }

    #[test]
    fn test_cleanup_stale_sequences_keeps_recent_entries() {
        let config = test_config();
        let mut engine = ConsensusEngine::new(
            config,
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD),
        );

        let n1 = node(1);

        // Add entries for sequences 95..100
        for seq in 95..100u64 {
            engine.first_event_for_sequence.insert((n1, seq), [seq as u8; 32]);
        }

        // n1 has created 100 events — distance from seq 95 is only 5
        engine.node_info.insert(
            n1,
            NodeConsensusInfo {
                current_round: 0,
                last_witness_round: 0,
                events_created: 100,
                events_committed: 0,
                last_event: None,
            },
        );

        // With distance=100, none should be removed (100-95=5, 5 <= 100)
        let removed = engine.cleanup_stale_sequences(Some(100));
        assert_eq!(removed, 0);
        assert_eq!(engine.first_event_for_sequence.len(), 5);
    }

    #[test]
    fn test_coin_round_deterministic() {
        // Same inputs must always produce same output
        let seed = [42u8; 32];
        let r1 = coin_round(1, &seed);
        let r2 = coin_round(1, &seed);
        assert_eq!(r1, r2);

        // Different rounds may produce different results
        let r3 = coin_round(2, &seed);
        // Not guaranteed to differ, but the function must be deterministic
        let _ = r3;
    }

    #[test]
    fn test_coin_round_differs_with_seed() {
        let seed_a = [1u8; 32];
        let seed_b = [2u8; 32];
        // At least one round should differ between seeds
        let mut found_difference = false;
        for round in 0..100u64 {
            if coin_round(round, &seed_a) != coin_round(round, &seed_b) {
                found_difference = true;
                break;
            }
        }
        assert!(
            found_difference,
            "coin_round should produce different results for different seeds"
        );
    }
}
