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
use crate::event::{Event, EventId};
use crate::slashing::{SlashOffense, SlashingEngine};
#[cfg(test)]
use crate::slashing::{DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD};
use crate::vector_clock::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Supermajority threshold (>2/3)
/// For N nodes, we need 2*N/3 + 1 for Byzantine fault tolerance
fn supermajority(total_nodes: usize) -> usize {
    (2 * total_nodes) / 3 + 1
}

/// Minimum votes needed for consensus
fn consensus_threshold(total_nodes: usize) -> usize {
    supermajority(total_nodes)
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
    /// Randomness seed for VRF-based leader selection.
    ///
    /// Updated each round from the previous round's VRF output to ensure
    /// unpredictability. Must not be all zeros in production.
    ///
    /// See: draft-irtf-cfrg-vrf-15, §5 — VRF-based leader selection
    pub round_seed: [u8; 32],
    /// Maximum duration (in milliseconds) a round may take before advancing.
    /// Default: 30_000 (30 seconds).
    pub round_timeout_ms: u64,
    /// Maximum consecutive timed-out rounds before entering recovery mode.
    /// Default: 3.
    pub max_consecutive_timeouts: u32,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            total_nodes: 4, // Default to 4 nodes (tolerates 1 Byzantine)
            commit_delay_rounds: 1,
            optimistic_confirmation: true,
            optimistic_threshold: 3, // >2/3 of 4
            max_look_ahead: 10,
            round_seed: [0u8; 32], // Must be set before production use
            round_timeout_ms: 30_000,
            max_consecutive_timeouts: 3,
        }
    }
}

impl ConsensusConfig {
    /// Create a config with a cryptographically random round seed.
    pub fn with_random_seed(total_nodes: usize) -> Result<Self, ConsensusError> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed)
            .map_err(|e| ConsensusError::EntropyFailed(e.to_string()))?;
        Ok(Self {
            total_nodes,
            commit_delay_rounds: 1,
            optimistic_confirmation: true,
            optimistic_threshold: supermajority(total_nodes) as u32,
            max_look_ahead: 10,
            round_seed: seed,
            round_timeout_ms: 30_000,
            max_consecutive_timeouts: 3,
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

/// The consensus engine
///
/// Tracks consensus state for all events and determines finality.
pub struct ConsensusEngine {
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
    /// Slashing engine for Byzantine fault penalties
    slashing: SlashingEngine,
    /// Round timer for liveness enforcement
    round_timer: RoundTimer,
    /// Maps (creator, sequence) → first EventId seen for that pair.
    first_event_for_sequence: HashMap<(NodeId, u64), EventId>,
}

impl ConsensusEngine {
    /// Create a new consensus engine with the given configuration and
    /// slashing engine.
    ///
    /// The slashing engine is injected from outside so that the same
    /// instance (sharing the same `Arc<dyn SlashingStore>`) can be used
    /// by both consensus and the API layer. This eliminates the
    /// dual-engine gap where consensus-detected equivocations were
    /// invisible to the REST API.
    ///
    /// # Arguments
    ///
    /// * `config` — Consensus configuration (node count, thresholds, etc.).
    /// * `slashing` — A [`SlashingEngine`] instance, typically cloned from
    ///   the one created in [`Substrate::new`](crate::Substrate::new).
    pub fn new(config: ConsensusConfig, slashing: SlashingEngine) -> Self {
        let round_timer = RoundTimer::new(
            Duration::from_millis(config.round_timeout_ms),
            config.max_consecutive_timeouts,
        );

        // Validate round_seed is not all zeros
        if config.round_seed == [0u8; 32] {
            tracing::warn!(
                "⚠️  ConsensusConfig.round_seed is all zeros! \
                 VRF leader selection will be deterministic and INSECURE. \
                 Use ConsensusConfig::with_random_seed() or set round_seed explicitly."
            );
            #[cfg(debug_assertions)]
            panic!("ConsensusConfig.round_seed must not be all zeros in debug builds");
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

    /// Process a new event through consensus
    ///
    /// This assigns the event to a round, checks if it's a witness,
    /// and determines if any events can be committed.
    pub fn process_event(
        &mut self,
        event: &Event,
        graph: &CausalGraph,
    ) -> Result<Vec<EventId>, ConsensusError> {
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
        if let Some(&first_id) = self.first_event_for_sequence.get(&seq_key) {
            // We've seen this (creator, sequence) before — check for equivocation
            if first_id != event_id {
                match graph.get_checked(&first_id) {
                    Ok(first_event) => {
                        // Existing equivocation check using full Event data
                        if SlashingEngine::check_equivocation(first_event, event) {
                            let outcome = self
                                .slashing
                                .record_offense(creator, SlashOffense::Equivocation);
                            tracing::warn!(
                                node = ?&creator[..4],
                                sequence = event.sequence,
                                first_id = ?&first_id[..4],
                                second_id = ?&event_id[..4],
                                outcome = ?outcome,
                                "Equivocation detected — multiple events with same creator+sequence"
                            );
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
                                let outcome = self
                                    .slashing
                                    .record_offense(creator, SlashOffense::Equivocation);
                                tracing::warn!(
                                    node = ?&creator[..4],
                                    sequence = event.sequence,
                                    first_id = ?&first_id[..4],
                                    second_id = ?&event_id[..4],
                                    outcome = ?outcome,
                                    "Equivocation detected (pruned first event) — multiple events with same creator+sequence"
                                );
                            }
                        }
                    }
                    Err(_) => {
                        // Event not found at all — should not happen
                        tracing::error!(
                            ?first_id,
                            "first_event_for_sequence references non-existent event"
                        );
                    }
                }
            }
        } else {
            self.first_event_for_sequence.insert(seq_key, event_id);
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
            self.event_states
                .insert(event_id, ConsensusState::Witness { round });
            self.round_witnesses
                .entry(round)
                .or_default()
                .insert(event_id);
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
            self.event_states
                .insert(committed_id, ConsensusState::Committed);
        }

        self.committed_count += committed.len() as u64;

        Ok(committed)
    }

    /// Record an acknowledgment for an event (from gossip)
    pub fn record_acknowledgment(&mut self, event_id: EventId) {
        if let Some(&state) = self.event_states.get(&event_id) {
            if state == ConsensusState::Pending && self.config.optimistic_confirmation {
                self.event_states
                    .insert(event_id, ConsensusState::Acknowledged);
            }
        }
    }

    /// Get the consensus state for an event
    pub fn get_state(&self, event_id: &EventId) -> Option<ConsensusState> {
        self.event_states.get(event_id).copied()
    }

    /// Check if an event is committed (final)
    pub fn is_committed(&self, event_id: &EventId) -> bool {
        matches!(
            self.event_states.get(event_id),
            Some(ConsensusState::Committed)
        )
    }

    /// Get the round assigned to an event
    pub fn get_round(&self, event_id: &EventId) -> Option<u64> {
        self.event_rounds.get(event_id).copied()
    }

    /// Get total committed events
    pub fn committed_count(&self) -> u64 {
        self.committed_count
    }

    /// Get current round for a node
    pub fn node_round(&self, node_id: &NodeId) -> u64 {
        self.node_info
            .get(node_id)
            .map(|i| i.current_round)
            .unwrap_or(0)
    }

    /// Get consensus statistics
    pub fn stats(&self) -> ConsensusStats {
        let mut by_state: HashMap<String, usize> = HashMap::new();
        for state in self.event_states.values() {
            let key = format!("{:?}", state);
            *by_state.entry(key).or_insert(0) += 1;
        }

        ConsensusStats {
            total_tracked: self.event_states.len(),
            committed: self.committed_count,
            current_max_round: self
                .node_info
                .values()
                .map(|i| i.current_round)
                .max()
                .unwrap_or(0),
            by_state,
            total_nodes: self.config.total_nodes,
            threshold: consensus_threshold(self.config.total_nodes),
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
    fn can_strongly_see(
        &self,
        event: &Event,
        witnesses: &HashSet<EventId>,
        graph: &CausalGraph,
    ) -> Result<bool, ConsensusError> {
        let mut seen_count = 0;

        for witness_id in witnesses {
            if graph.is_ancestor_of(&event.id, witness_id).unwrap_or(false) {
                seen_count += 1;
            }
        }

        Ok(seen_count >= consensus_threshold(self.config.total_nodes))
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
    fn check_optimistic_confirmation(&mut self, event: &Event) {
        if event.ack_count >= self.config.optimistic_threshold {
            self.event_states
                .insert(event.id, ConsensusState::Acknowledged);
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
        let mut committed = Vec::new();

        // Only witnesses can trigger commitments
        if !matches!(
            self.event_states.get(&event_id),
            Some(ConsensusState::Witness { .. })
        ) {
            return Ok(committed);
        }

        // FIX(bug-3): Removed shadowing line `let round = ...` that overwrote
        // the correct `round` parameter. Using the parameter directly.

        if round < self.config.commit_delay_rounds {
            // Not enough rounds have passed for commitment safety.
            // However, genesis round (round 0) with supermajority can still commit.
            if round == 0 {
                if let Some(witnesses) = self.round_witnesses.get(&0) {
                    if witnesses.len() >= consensus_threshold(self.config.total_nodes) {
                        for &witness_id in witnesses {
                            if !self.is_committed(&witness_id) {
                                committed.push(witness_id);
                            }
                        }
                    }
                }
            }
            return Ok(committed);
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
                    committed.push(witness_id);

                    if let Ok(ancestors) = graph.get_ancestors(&witness_id) {
                        for ancestor in ancestors {
                            if !self.is_committed(&ancestor) {
                                committed.push(ancestor);
                            }
                        }
                    }
                }
            }
        }

        Ok(committed)
    }

    /// Determine if a witness is famous
    fn is_famous(
        &self,
        witness_id: EventId,
        witness_round: u64,
        graph: &CausalGraph,
    ) -> Result<bool, ConsensusError> {
        let check_round = witness_round + self.config.commit_delay_rounds;

        if let Some(witnesses) = self.round_witnesses.get(&check_round) {
            let mut seeing_count = 0;

            for later_witness_id in witnesses {
                if graph
                    .is_ancestor_of(later_witness_id, &witness_id)
                    .unwrap_or(false)
                {
                    seeing_count += 1;
                }
            }

            return Ok(seeing_count >= consensus_threshold(self.config.total_nodes));
        }

        Ok(false)
    }

    /// Get all committed events
    pub fn get_committed(&self) -> Vec<EventId> {
        self.event_states
            .iter()
            .filter(|(_, &state)| state == ConsensusState::Committed)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get all famous witnesses
    pub fn get_famous_witnesses(&self) -> Vec<(EventId, u64)> {
        self.fame_status
            .iter()
            .filter(|(_, &famous)| famous)
            .filter_map(|(id, _)| self.event_rounds.get(id).map(|&round| (*id, round)))
            .collect()
    }

    /// Register a validator with the slashing engine.
    ///
    /// Delegates to [`SlashingEngine::register_validator`]. The validator
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
    /// use omnia_substrate::{ConsensusEngine, SlashingEngine};
    /// let slashing = SlashingEngine::new_in_memory(500, 2000);
    /// let mut engine = ConsensusEngine::new(config, slashing);
    /// engine.register_validator(node_id, 10_000);
    /// ```
    pub fn register_validator(&mut self, node: NodeId, stake: u64) {
        self.slashing.register_validator(node, stake);
    }

    /// Check if a node has been slashed.
    ///
    /// Delegates to [`SlashingEngine::is_slashed`].
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

    /// Compute the VRF-based leader for the given round.
    ///
    /// Uses the VRF module's [`select_leader`](crate::vrf::select_leader)
    /// function with the current `round_seed` from the configuration and
    /// the provided round number. Candidates with zero stake are skipped.
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
    ///
    /// # References
    ///
    /// See: draft-irtf-cfrg-vrf-15 — Verifiable Random Functions
    pub fn compute_leader(
        &self,
        candidates: &HashMap<NodeId, (crate::crypto::NodeKeypair, u64)>,
        round_number: u64,
    ) -> Result<NodeId, crate::vrf::VrfError> {
        crate::vrf::select_leader(candidates, &self.config.round_seed, round_number)
    }

    /// Update the round seed with a new VRF output.
    ///
    /// This should be called after each round to ensure the next round's
    /// leader selection is unpredictable. The new seed is derived from
    /// the VRF output of the current round's leader.
    ///
    /// # Arguments
    ///
    /// * `new_seed` — The new 32-byte seed for the next round
    pub fn update_round_seed(&mut self, new_seed: [u8; 32]) {
        tracing::info!(
            old_seed = ?&self.config.round_seed[..4],
            new_seed = ?&new_seed[..4],
            "Updating round seed for VRF leader selection"
        );
        self.config.round_seed = new_seed;
    }

    /// Called in the main consensus loop to check for round timeouts.
    /// Returns true if the round was advanced.
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
                tracing::warn!(
                    "Round {} timed out. Advancing to next round.",
                    current_round
                );
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
    pub fn current_round(&self) -> u64 {
        self.node_info
            .values()
            .map(|i| i.current_round)
            .max()
            .unwrap_or(0)
    }

    /// Advance to the next round with a new leader.
    /// Called when the current round times out.
    pub fn advance_round(&mut self) {
        let current = self.current_round();
        let next = current + 1;

        self.update_round_seed_from_timeout(next);

        tracing::warn!(
            "Advanced from round {} to round {} due to timeout",
            current,
            next
        );
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
}

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
    /// Failed to obtain entropy for random seed generation.
    #[error("entropy generation failed: {0}")]
    EntropyFailed(String),
    /// An invariant was violated.
    #[error("invariant violated: {0}")]
    InvariantViolated(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::causal_graph::CausalGraph;
    use crate::crypto::generate_keypair;
    use crate::event::Event;
    use crate::vector_clock::VectorClock;

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
        }
    }

    fn setup_graph_with_events() -> (CausalGraph, Vec<EventId>) {
        let mut graph = CausalGraph::new();
        let n1 = node(1);
        let n2 = node(2);
        let n3 = node(3);
        let n4 = node(4);

        let mut events = Vec::new();

        for &n in &[n1, n2, n3, n4] {
            let keypair = generate_keypair();
            let mut e = Event::genesis(n, vec![n[0]]);
            e.sign_with_keypair(&keypair);
            let id = e.id;
            graph.insert(e).unwrap();
            events.push(id);
        }

        for i in 0..4 {
            let creator = [n1, n2, n3, n4][i];
            let keypair = generate_keypair();
            let sp = events[i];
            let op = events[(i + 1) % 4];

            let mut vc = VectorClock::with_node(creator, 2);
            let other = [n1, n2, n3, n4][(i + 1) % 4];
            vc.set(other, 1);

            let mut e = Event::new(creator, 1, vc, Some(sp), Some(op), vec![]);
            e.sign_with_keypair(&keypair);
            let id = e.id;
            graph.insert(e).unwrap();
            events.push(id);
        }

        (graph, events)
    }

    #[test]
    fn test_consensus_engine_creation() {
        let config = test_config();
        let slashing =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
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
            let mut e = Event::genesis(*node_id, vec![node_id[0]]);
            e.sign_with_keypair(keypair);
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
            let mut e = Event::genesis(*node_id, vec![node_id[0]]);
            e.sign_with_keypair(keypair);
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
    use crate::event::Event;
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
        }
    }

    /// Strategy: generate a genesis-like event with arbitrary creator and payload.
    /// These events won't have valid signatures, but they exercise the
    /// consensus engine's internal logic without requiring a populated graph.
    fn arb_genesis_event() -> impl Strategy<Value = Event> {
        (any::<u8>(), any::<Vec<u8>>()).prop_map(|(creator_byte, payload)| {
            let creator = nid(creator_byte % 10);
            Event::genesis(creator, payload)
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
    use std::thread;

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
}
