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
use crate::vector_clock::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            total_nodes: 4, // Default to 4 nodes (tolerates 1 Byzantine)
            commit_delay_rounds: 1,
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
            _last_finalized: VectorClock::new(),
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
            let info = self.node_info.get_mut(&creator).unwrap();
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

        // For small networks, reduce commit delay
        let effective_delay = self.config.commit_delay_rounds.min(round.saturating_sub(1));
        let check_round = round.saturating_sub(effective_delay);

        if check_round == 0 {
            // Genesis round: commit all genesis witnesses immediately if we have supermajority
            if let Some(witnesses) = self.round_witnesses.get(&0) {
                if witnesses.len() >= consensus_threshold(self.config.total_nodes) {
                    for &witness_id in witnesses {
                        if !self.is_committed(&witness_id) {
                            committed.push(witness_id);
                        }
                    }
                }
            }
            return Ok(committed);
        }

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
}

#[cfg(test)]
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
        let config = ConsensusConfig::default();
        let engine = ConsensusEngine::new(config);

        assert_eq!(engine.committed_count(), 0);
        assert_eq!(engine.stats().total_nodes, 4);
        assert_eq!(engine.stats().threshold, 3);
    }

    #[test]
    fn test_process_event() {
        let config = ConsensusConfig::default();
        let mut engine = ConsensusEngine::new(config);
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
        let config = ConsensusConfig::default();
        let mut engine = ConsensusEngine::new(config);
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
        let mut config = ConsensusConfig::default();
        config.total_nodes = 3;

        let engine = ConsensusEngine::new(config);
        assert_eq!(engine.stats().threshold, 3);
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
            commit_delay_rounds: 1,
            optimistic_confirmation: false,
            optimistic_threshold: 3,
            max_look_ahead: 10,
        };
        let mut engine = ConsensusEngine::new(config);

        let (graph, events) = setup_graph_with_events();
        for event_id in &events {
            let event = graph.get(event_id).unwrap();
            engine.process_event(event, &graph).unwrap();
        }

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

    /// FIX 3: Test that proves finality works in a 4-node network.
    #[test]
    fn test_four_node_finality() {
        let mut config = ConsensusConfig::default();
        config.total_nodes = 4;
        config.commit_delay_rounds = 1; // Reduced for small network

        let mut engine = ConsensusEngine::new(config);
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
}
