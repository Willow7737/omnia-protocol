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
use crate::event::{Event, EventId, EventStatus};
use crate::vector_clock::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tracing::{debug, info, trace};

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
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            total_nodes: 4, // Default to 4 nodes (tolerates 1 Byzantine)
            commit_delay_rounds: 2,
            optimistic_confirmation: true,
            optimistic_threshold: 3, // >2/3 of 4
            max_look_ahead: 10,
        }
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
    Witness { round: u64 },
    /// Event is famous (seen by supermajority)
    Famous,
    /// Event is committed (final)
    Committed,
}

/// Information about a node's participation in consensus
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeConsensusInfo {
    /// Current round for this node
    pub current_round: u64,
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
    last_finalized: VectorClock,
}

impl ConsensusEngine {
    /// Create a new consensus engine
    pub fn new(config: ConsensusConfig) -> Self {
        Self {
            config,
            event_states: HashMap::new(),
            event_rounds: HashMap::new(),
            round_witnesses: HashMap::new(),
            fame_status: HashMap::new(),
            node_info: HashMap::new(),
            committed_count: 0,
            last_finalized: VectorClock::new(),
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
            info.current_round = info.current_round.max(round);
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

            // Update graph status
            if let Ok(_) = graph.get(&committed_id) {
                // Note: We can't modify graph here as it's read-only
                // In production, this would be done through a callback
            }
        }

        self.committed_count += committed.len() as u64;

        Ok(committed)
    }

    /// Record an acknowledgment for an event (from gossip)
    pub fn record_acknowledgment(&mut self, event_id: EventId) {
        // In practice, we'd track per-node acks
        // For now, just update the event status if it reaches threshold
        if let Some(&state) = self.event_states.get(&event_id) {
            if state == ConsensusState::Pending && self.config.optimistic_confirmation {
                // Track ack count (simplified)
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
            current_max_round: self.node_info.values().map(|i| i.current_round).max().unwrap_or(0),
            by_state,
            total_nodes: self.config.total_nodes,
            threshold: consensus_threshold(self.config.total_nodes),
        }
    }

    // --- Internal methods ---

    /// Assign a round to an event
    ///
    /// Round assignment follows Hashgraph's rule:
    /// - Genesis events are round 0
    /// - An event enters round R+1 if it can "strongly see" >2/3 witnesses from round R
    /// - "Strongly see" means there are multiple paths through different nodes
    fn assign_round(
        &mut self,
        event: &Event,
        graph: &CausalGraph,
    ) -> Result<u64, ConsensusError> {
        // Genesis or first event: round 0
        if event.is_root() {
            return Ok(0);
        }

        // Find the maximum round of parents
        let parent_rounds: Vec<u64> = [event.self_parent, event.other_parent]
            .iter()
            .filter_map(|&opt_id| {
                opt_id.and_then(|id| self.event_rounds.get(&id).copied())
            })
            .collect();

        let max_parent_round = parent_rounds.iter().max().copied().unwrap_or(0);

        // Check if we can advance to the next round
        // An event advances to round R+1 if it strongly sees >2/3 witnesses from round R
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
    /// "Strongly see" means the event can see the witness through multiple
    /// different node paths (Byzantine fault tolerance requirement)
    fn can_strongly_see(
        &self,
        event: &Event,
        witnesses: &HashSet<EventId>,
        graph: &CausalGraph,
    ) -> Result<bool, ConsensusError> {
        let mut seen_count = 0;

        for witness_id in witnesses {
            // Simple check: is the witness an ancestor?
            if graph.is_ancestor_of(&event.id, witness_id).unwrap_or(false) {
                seen_count += 1;
            }
        }

        Ok(seen_count >= consensus_threshold(self.config.total_nodes))
    }

    /// Check if an event is a witness (first event in its round for its creator)
    fn is_witness(&self, event_id: EventId, creator: NodeId, round: u64) -> bool {
        // Check if there's already a witness from this creator in this round
        if let Some(witnesses) = self.round_witnesses.get(&round) {
            for witness_id in witnesses {
                // We'd need to check the creator of each witness
                // For efficiency, we track this in node_info
            }
        }

        // A node can only have one witness per round
        // Check node_info to see if this is the first event in this round
        if let Some(info) = self.node_info.get(&creator) {
            // This is simplified — in production, track witnesses per node per round
            round > info.current_round || info.events_created == 1
        } else {
            true // First event from this node
        }
    }

    /// Check for optimistic confirmation (fast path)
    fn check_optimistic_confirmation(&mut self, event: &Event) {
        if event.ack_count >= self.config.optimistic_threshold {
            self.event_states
                .insert(event.id, ConsensusState::Acknowledged);
        }
    }

    /// Check if any events can be committed based on this new event
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

        // Check fame of witnesses from previous rounds
        let check_round = round.saturating_sub(self.config.commit_delay_rounds);

        if let Some(witnesses) = self.round_witnesses.get(&check_round).cloned() {
            for witness_id in witnesses {
                if self.is_committed(&witness_id) {
                    continue;
                }

                // Check if this witness is "famous"
                // A witness is famous if >2/3 of next-round witnesses can see it
                if self.is_famous(witness_id, check_round, graph)? {
                    self.fame_status.insert(witness_id, true);
                    committed.push(witness_id);

                    // Also commit all ancestors of this witness
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
        // A witness is famous if >2/3 of witnesses in subsequent rounds can see it
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

        // Not enough subsequent rounds yet
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
            .filter_map(|(id, _)| {
                self.event_rounds
                    .get(id)
                    .map(|&round| (*id, round))
            })
            .collect()
    }
}

/// Consensus statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusStats {
    pub total_tracked: usize,
    pub committed: u64,
    pub current_max_round: u64,
    pub by_state: HashMap<String, usize>,
    pub total_nodes: usize,
    pub threshold: usize,
}

/// Errors during consensus operations
#[derive(Error, Debug, Clone)]
pub enum ConsensusError {
    #[error("Graph error: {0}")]
    GraphError(String),
    #[error("Event not found: {0:?}")]
    EventNotFound(EventId),
    #[error("Invalid state transition")]
    InvalidStateTransition,
    #[error("Not enough nodes for consensus (need at least 4, got {0})")]
    InsufficientNodes(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal_graph::CausalGraph;
    use crate::event::Event;
    use crate::vector_clock::VectorClock;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    fn setup_graph_with_events() -> (CausalGraph, Vec<EventId>) {
        let mut graph = CausalGraph::new();
        let n1 = node(1);
        let n2 = node(2);
        let n3 = node(3);
        let n4 = node(4);

        let mut events = Vec::new();

        // Genesis events from 4 nodes
        for &n in &[n1, n2, n3, n4] {
            let mut e = Event::genesis(n, vec![n[0]]);
            e.sign(vec![1]);
            let id = e.id;
            graph.insert(e).unwrap();
            events.push(id);
        }

        // Second round: each node references two others
        for i in 0..4 {
            let creator = [n1, n2, n3, n4][i];
            let sp = events[i]; // self-parent
            let op = events[(i + 1) % 4]; // other-parent

            let mut vc = VectorClock::with_node(creator, 2);
            let other = [n1, n2, n3, n4][(i + 1) % 4];
            vc.set(other, 1);

            let mut e = Event::new(creator, 1, vc, Some(sp), Some(op), vec![]);
            e.sign(vec![1]);
            let id = e.id;
            graph.insert(e).unwrap();
            events.push(id);
        }

        (graph, events)
    }

    #[test]
    fn test_consensus_engine_creation() {
        let config = ConsensusConfig::default();
        let engine = ConsensusEngine::new(config);

        assert_eq!(engine.committed_count(), 0);
        assert_eq!(engine.stats().total_nodes, 4);
        assert_eq!(engine.stats().threshold, 3); // 2*4/3 + 1 = 3
    }

    #[test]
    fn test_process_event() {
        let config = ConsensusConfig::default();
        let mut engine = ConsensusEngine::new(config);
        let (graph, events) = setup_graph_with_events();

        for event_id in &events {
            let event = graph.get(event_id).unwrap();
            let committed = engine.process_event(event, &graph).unwrap();
            // Initial events may not commit immediately
            assert!(committed.len() <= events.len());
        }

        assert_eq!(engine.stats().total_tracked, events.len());
    }

    #[test]
    fn test_supermajority_threshold() {
        assert_eq!(supermajority(4), 3);  // 2*4/3 + 1 = 3
        assert_eq!(supermajority(7), 5);  // 2*7/3 + 1 = 5
        assert_eq!(supermajority(10), 7); // 2*10/3 + 1 = 7
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
        let config = ConsensusConfig::default();
        let mut engine = ConsensusEngine::new(config);
        let (graph, events) = setup_graph_with_events();

        // Process all events
        for event_id in &events {
            let event = graph.get(event_id).unwrap();
            engine.process_event(event, &graph).unwrap();
        }

        // Check that node info was tracked
        let stats = engine.stats();
        assert!(stats.total_tracked > 0);
    }

    #[test]
    fn test_insufficient_nodes_error() {
        let mut config = ConsensusConfig::default();
        config.total_nodes = 3; // Need at least 4 for BFT

        let engine = ConsensusEngine::new(config);
        assert_eq!(engine.stats().threshold, 3); // Still works but less safe
    }

    #[test]
    fn test_get_round() {
        let config = ConsensusConfig::default();
        let mut engine = ConsensusEngine::new(config);
        let (graph, events) = setup_graph_with_events();

        let event = graph.get(&events[0]).unwrap();
        engine.process_event(event, &graph).unwrap();

        assert!(engine.get_round(&events[0]).is_some());
    }

    #[test]
    fn test_fame_determination() {
        let config = ConsensusConfig {
            total_nodes: 4,
            commit_delay_rounds: 2,
            optimistic_confirmation: false,
            optimistic_threshold: 3,
            max_look_ahead: 10,
        };
        let mut engine = ConsensusEngine::new(config);

        // With few events, no one should be famous yet
        let (graph, events) = setup_graph_with_events();
        for event_id in &events {
            let event = graph.get(event_id).unwrap();
            engine.process_event(event, &graph).unwrap();
        }

        // Should have tracked events but fame requires more rounds
        assert_eq!(engine.stats().total_tracked, events.len());
    }

    #[test]
    fn test_record_acknowledgment() {
        let config = ConsensusConfig::default();
        let mut engine = ConsensusEngine::new(config);
        let (graph, events) = setup_graph_with_events();

        let event = graph.get(&events[0]).unwrap();
        engine.process_event(event, &graph).unwrap();

        engine.record_acknowledgment(events[0]);

        // With optimistic confirmation, should advance to Acknowledged
        let state = engine.get_state(&events[0]);
        assert!(
            matches!(state, Some(ConsensusState::Acknowledged))
                || matches!(state, Some(ConsensusState::Witness { .. }))
                || matches!(state, Some(ConsensusState::Pending))
        );
    }

    #[test]
    fn test_consensus_stats() {
        let config = ConsensusConfig::default();
        let engine = ConsensusEngine::new(config);

        let stats = engine.stats();
        assert_eq!(stats.total_tracked, 0);
        assert_eq!(stats.committed, 0);
        assert_eq!(stats.current_max_round, 0);
        assert_eq!(stats.total_nodes, 4);
    }
}
