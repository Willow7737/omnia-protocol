//! Causal Graph (DAG) Implementation
//!
//! The CausalGraph is the core data structure of Omnia Layer 1. It stores all events
//! as nodes in a directed acyclic graph, with edges representing parent relationships.
//!
//! Key capabilities:
//! - O(1) event insertion and lookup
//! - O(k) ancestry traversal where k is path length
//! - Concurrent event detection for parallel execution
//! - Topological ordering for deterministic event sequencing
//! - Diff calculation for efficient synchronization

use crate::event::{Event, EventHeader, EventId, EventStatus};
use crate::vector_clock::{CausalOrder, NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

/// Maximum depth for ancestry traversal (prevents infinite loops from corrupted data)
const MAX_ANCESTRY_DEPTH: usize = 1_000_000;

/// Maximum number of tips to track before forced consolidation
const MAX_TIPS: usize = 10_000;

/// Errors that can occur during causal graph operations
#[derive(Error, Debug, Clone, PartialEq)]
pub enum CausalGraphError {
    #[error("Event {0} already exists in graph")]
    DuplicateEvent(String),
    #[error("Parent event {0} not found")]
    MissingParent(String),
    #[error("Cycle detected — cannot add event {0}")]
    CycleDetected(String),
    #[error("Maximum ancestry depth exceeded starting from {0}")]
    MaxDepthExceeded(String),
    #[error("Invalid event: {0}")]
    InvalidEvent(String),
    #[error("Graph integrity check failed: {0}")]
    IntegrityError(String),
}

/// Statistics about the causal graph
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_events: usize,
    pub tip_count: usize,
    pub node_count: usize,
    pub max_depth: usize,
    pub finalized_events: usize,
    pub pending_events: usize,
}

/// The core causal graph — a DAG of events
///
/// Each event is stored once and indexed by its hash. Parent relationships
/// form the directed edges. The graph maintains:
/// - All events by ID
/// - Current tips (events with no children)
/// - Per-node highest sequence number
/// - The frontier (latest known vector clock)
pub struct CausalGraph {
    /// All events in the graph, keyed by event ID
    events: HashMap<EventId, Event>,
    /// Index of events by creator for efficient lookup
    by_creator: HashMap<NodeId, Vec<EventId>>,
    /// Current tips (events not yet referenced as parents)
    tips: HashSet<EventId>,
    /// Highest sequence number seen per node
    node_sequences: HashMap<NodeId, u64>,
    /// The current frontier vector clock (latest known state)
    frontier: VectorClock,
    /// Maximum depth reached in the graph
    max_depth: usize,
    /// Number of finalized events
    finalized_count: usize,
}

impl CausalGraph {
    /// Create a new, empty causal graph
    pub fn new() -> Self {
        Self {
            events: HashMap::new(),
            by_creator: HashMap::new(),
            tips: HashSet::new(),
            node_sequences: HashMap::new(),
            frontier: VectorClock::new(),
            max_depth: 0,
            finalized_count: 0,
        }
    }

    /// Insert a new event into the graph
    ///
    /// # Arguments
    /// * `event` - The event to insert (must have valid hash and signature)
    ///
    /// # Errors
    /// * `DuplicateEvent` — Event already exists
    /// * `MissingParent` — One or both parents not in graph
    /// * `CycleDetected` — Adding this event would create a cycle
    pub fn insert(&mut self, event: Event) -> Result<(), CausalGraphError> {
        let event_id = event.id;

        // Check for duplicate
        if self.events.contains_key(&event_id) {
            return Err(CausalGraphError::DuplicateEvent(format!(
                "{}",
                hex::encode(&event_id[..8])
            )));
        }

        // Validate event hash
        if !event.verify_hash() {
            return Err(CausalGraphError::InvalidEvent(format!(
                "hash mismatch for {}",
                hex::encode(&event_id[..8])
            )));
        }

        // Verify parents exist (except for genesis events)
        if let Some(sp) = event.self_parent {
            if !self.events.contains_key(&sp) {
                return Err(CausalGraphError::MissingParent(format!(
                    "self-parent {}",
                    hex::encode(&sp[..8])
                )));
            }
        }
        if let Some(op) = event.other_parent {
            if !self.events.contains_key(&op) {
                return Err(CausalGraphError::MissingParent(format!(
                    "other-parent {}",
                    hex::encode(&op[..8])
                )));
            }
        }

        // Check for cycles: ensure the event doesn't reference itself transitively
        if let Some(sp) = event.self_parent {
            if self.is_ancestor_of(&event_id, &sp)? {
                return Err(CausalGraphError::CycleDetected(format!(
                    "{}",
                    hex::encode(&event_id[..8])
                )));
            }
        }
        if let Some(op) = event.other_parent {
            if self.is_ancestor_of(&event_id, &op)? {
                return Err(CausalGraphError::CycleDetected(format!(
                    "{}",
                    hex::encode(&event_id[..8])
                )));
            }
        }

        // Remove parents from tips (they now have children)
        if let Some(sp) = event.self_parent {
            self.tips.remove(&sp);
        }
        if let Some(op) = event.other_parent {
            self.tips.remove(&op);
        }

        // Update creator index
        self.by_creator
            .entry(event.creator)
            .or_default()
            .push(event_id);

        // Update node sequence tracking
        let current_seq = self.node_sequences.entry(event.creator).or_insert(0);
        *current_seq = (*current_seq).max(event.sequence);

        // Update frontier vector clock
        self.frontier.merge(&event.vector_clock);

        // Add to tips
        self.tips.insert(event_id);

        // Update max depth
        let depth = self.calculate_depth(&event_id)?;
        self.max_depth = self.max_depth.max(depth);

        // Track finalized count
        if event.status == EventStatus::Finalized {
            self.finalized_count += 1;
        }

        // Store the event
        self.events.insert(event_id, event);

        // Prune tips if too many
        if self.tips.len() > MAX_TIPS {
            self.consolidate_tips();
        }

        Ok(())
    }

    /// Get an event by its ID
    pub fn get(&self, event_id: &EventId) -> Option<&Event> {
        self.events.get(event_id)
    }

    /// Get a mutable reference to an event
    pub fn get_mut(&mut self, event_id: &EventId) -> Option<&mut Event> {
        self.events.get_mut(event_id)
    }

    /// Check if an event exists in the graph
    pub fn contains(&self, event_id: &EventId) -> bool {
        self.events.contains_key(event_id)
    }

    /// Get the number of events in the graph
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if the graph is empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get all current tips
    pub fn tips(&self) -> impl Iterator<Item = &EventId> {
        self.tips.iter()
    }

    /// Get the current frontier vector clock
    pub fn frontier(&self) -> &VectorClock {
        &self.frontier
    }

    /// Get all events created by a specific node
    pub fn by_creator(&self, node_id: &NodeId) -> Vec<&Event> {
        self.by_creator
            .get(node_id)
            .map(|ids| ids.iter().filter_map(|id| self.events.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get the latest sequence number for a node
    pub fn latest_sequence(&self, node_id: &NodeId) -> u64 {
        self.node_sequences.get(node_id).copied().unwrap_or(0)
    }

    /// Check if `ancestor` is an ancestor of `descendant` (causal path exists)
    pub fn is_ancestor_of(
        &self,
        descendant: &EventId,
        ancestor: &EventId,
    ) -> Result<bool, CausalGraphError> {
        if descendant == ancestor {
            return Ok(false); // An event is not its own ancestor
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(*descendant);
        visited.insert(*descendant);

        let mut depth = 0;

        while let Some(current_id) = queue.pop_front() {
            depth += 1;
            if depth > MAX_ANCESTRY_DEPTH {
                return Err(CausalGraphError::MaxDepthExceeded(format!(
                    "{}",
                    hex::encode(&descendant[..8])
                )));
            }

            if let Some(event) = self.events.get(&current_id) {
                for parent in [event.self_parent, event.other_parent].iter().flatten() {
                    if parent == ancestor {
                        return Ok(true);
                    }
                    if visited.insert(*parent) {
                        queue.push_back(*parent);
                    }
                }
            }
        }

        Ok(false)
    }

    /// Get all ancestors of an event (causal history)
    pub fn get_ancestors(&self, event_id: &EventId) -> Result<HashSet<EventId>, CausalGraphError> {
        let mut ancestors = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(*event_id);

        let mut depth = 0;

        while let Some(current_id) = queue.pop_front() {
            depth += 1;
            if depth > MAX_ANCESTRY_DEPTH {
                return Err(CausalGraphError::MaxDepthExceeded(format!(
                    "{}",
                    hex::encode(&event_id[..8])
                )));
            }

            if let Some(event) = self.events.get(&current_id) {
                for parent in [event.self_parent, event.other_parent].iter().flatten() {
                    if ancestors.insert(*parent) {
                        queue.push_back(*parent);
                    }
                }
            }
        }

        Ok(ancestors)
    }

    /// Calculate the depth of an event (longest path from genesis)
    fn calculate_depth(&self, event_id: &EventId) -> Result<usize, CausalGraphError> {
        let mut memo = HashMap::new();
        self.calculate_depth_memo(event_id, &mut memo)
    }

    fn calculate_depth_memo(
        &self,
        event_id: &EventId,
        memo: &mut HashMap<EventId, usize>,
    ) -> Result<usize, CausalGraphError> {
        if let Some(&depth) = memo.get(event_id) {
            return Ok(depth);
        }

        let event = self
            .events
            .get(event_id)
            .ok_or_else(|| CausalGraphError::InvalidEvent("event not found".to_string()))?;

        let mut max_parent_depth = 0;
        for parent in [event.self_parent, event.other_parent].iter().flatten() {
            max_parent_depth = max_parent_depth.max(self.calculate_depth_memo(parent, memo)?);
        }

        let depth = max_parent_depth + 1;
        memo.insert(*event_id, depth);
        Ok(depth)
    }

    /// Find events that are concurrent with the given event (independent, parallelizable)
    pub fn find_concurrent(&self, event_id: &EventId) -> Vec<&Event> {
        let Some(event) = self.events.get(event_id) else {
            return Vec::new();
        };

        self.events
            .values()
            .filter(|other| {
                other.id != *event_id && other.vector_clock.concurrent(&event.vector_clock)
            })
            .collect()
    }

    /// Get a topological ordering of events from a starting set
    /// Uses Kahn's algorithm for deterministic ordering
    pub fn topological_order(&self, start_from: Option<&VectorClock>) -> Vec<EventId> {
        let relevant_events: Vec<&Event> = match start_from {
            Some(vc) => self
                .events
                .values()
                .filter(|e| e.vector_clock.happened_after(vc) || e.vector_clock.concurrent(vc))
                .collect(),
            None => self.events.values().collect(),
        };

        let mut in_degree: HashMap<EventId, usize> = HashMap::new();
        let mut children: HashMap<EventId, Vec<EventId>> = HashMap::new();

        // Initialize in-degree for relevant events
        for event in &relevant_events {
            in_degree.entry(event.id).or_insert(0);
        }

        // Count in-degrees among relevant events only
        for event in &relevant_events {
            for parent in [event.self_parent, event.other_parent].iter().flatten() {
                if in_degree.contains_key(parent) {
                    children.entry(*parent).or_default().push(event.id);
                    *in_degree.entry(event.id).or_insert(0) += 1;
                }
            }
        }

        // Start with events that have no parents in the relevant set
        let mut queue: VecDeque<EventId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| *id)
            .collect();

        // Sort for determinism
        let mut queue_vec: Vec<EventId> = queue.into_iter().collect();
        queue_vec.sort_by(|a, b| {
            let event_a = self.events.get(a).unwrap();
            let event_b = self.events.get(b).unwrap();
            event_a
                .timestamp
                .cmp(&event_b.timestamp)
                .then_with(|| a.cmp(b))
        });
        queue = VecDeque::from(queue_vec);

        let mut result = Vec::new();

        while let Some(current_id) = queue.pop_front() {
            result.push(current_id);

            if let Some(child_ids) = children.get(&current_id) {
                for child_id in child_ids {
                    if let Some(deg) = in_degree.get_mut(child_id) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(*child_id);
                        }
                    }
                }
            }
        }

        result
    }

    /// Find events that are in our graph but not in the given set (for sync)
    pub fn diff(&self, known_events: &HashSet<EventId>) -> Vec<&Event> {
        self.events
            .values()
            .filter(|e| !known_events.contains(&e.id))
            .collect()
    }

    /// Find events newer than a given vector clock (for sync)
    pub fn since(&self, clock: &VectorClock) -> Vec<&Event> {
        self.events
            .values()
            .filter(|e| e.vector_clock.happened_after(clock))
            .collect()
    }

    /// Get graph statistics
    pub fn stats(&self) -> GraphStats {
        GraphStats {
            total_events: self.events.len(),
            tip_count: self.tips.len(),
            node_count: self.node_sequences.len(),
            max_depth: self.max_depth,
            finalized_events: self.finalized_count,
            pending_events: self
                .events
                .values()
                .filter(|e| {
                    e.status == EventStatus::Pending || e.status == EventStatus::Gossiped
                })
                .count(),
        }
    }

    /// Mark an event as finalized
    pub fn finalize_event(&mut self, event_id: &EventId) -> Result<(), CausalGraphError> {
        if let Some(event) = self.events.get_mut(event_id) {
            if event.status != EventStatus::Finalized {
                event.status = EventStatus::Finalized;
                self.finalized_count += 1;
            }
            Ok(())
        } else {
            Err(CausalGraphError::InvalidEvent(format!(
                "event not found: {}",
                hex::encode(&event_id[..8])
            )))
        }
    }

    /// Get all finalized events in topological order
    pub fn finalized_order(&self) -> Vec<&Event> {
        let finalized: Vec<EventId> = self
            .events
            .values()
            .filter(|e| e.status == EventStatus::Finalized)
            .map(|e| e.id)
            .collect();

        let order = self.topological_order(None);
        order
            .into_iter()
            .filter(|id| finalized.contains(id))
            .filter_map(|id| self.events.get(&id))
            .collect()
    }

    /// Consolidate old tips by removing oldest (simple strategy)
    fn consolidate_tips(&mut self) {
        if self.tips.len() <= MAX_TIPS {
            return;
        }

        let mut tip_events: Vec<&Event> = self
            .tips
            .iter()
            .filter_map(|id| self.events.get(id))
            .collect();

        // Sort by timestamp (oldest first)
        tip_events.sort_by_key(|e| e.timestamp);

        // Remove oldest tips beyond threshold
        let to_remove = self.tips.len() - MAX_TIPS + MAX_TIPS / 10; // Remove 10% extra
        for event in tip_events.into_iter().take(to_remove) {
            self.tips.remove(&event.id);
        }
    }

    /// Verify graph integrity (no dangling references, no cycles)
    pub fn verify_integrity(&self) -> Result<(), CausalGraphError> {
        for (id, event) in &self.events {
            // Check self-parent exists
            if let Some(sp) = event.self_parent {
                if !self.events.contains_key(&sp) {
                    return Err(CausalGraphError::IntegrityError(format!(
                        "event {} has dangling self-parent {}",
                        hex::encode(&id[..8]),
                        hex::encode(&sp[..8])
                    )));
                }
            }
            // Check other-parent exists
            if let Some(op) = event.other_parent {
                if !self.events.contains_key(&op) {
                    return Err(CausalGraphError::IntegrityError(format!(
                        "event {} has dangling other-parent {}",
                        hex::encode(&id[..8]),
                        hex::encode(&op[..8])
                    )));
                }
            }
            // Verify hash
            if !event.verify_hash() {
                return Err(CausalGraphError::IntegrityError(format!(
                    "event {} has invalid hash",
                    hex::encode(&id[..8])
                )));
            }
        }

        // Check for cycles (from each tip, walk back)
        for tip in &self.tips {
            let mut visited = HashSet::new();
            let mut queue = VecDeque::new();
            queue.push_back(*tip);

            let mut depth = 0;
            while let Some(current) = queue.pop_front() {
                depth += 1;
                if depth > MAX_ANCESTRY_DEPTH * 2 {
                    return Err(CausalGraphError::IntegrityError(
                        "possible cycle detected from tip".to_string(),
                    ));
                }
                if !visited.insert(current) {
                    return Err(CausalGraphError::IntegrityError(
                        "cycle found in graph".to_string(),
                    ));
                }
                if let Some(event) = self.events.get(&current) {
                    for parent in [event.self_parent, event.other_parent].iter().flatten() {
                        queue.push_back(*parent);
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for CausalGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// A read-only snapshot of the causal graph for concurrent access
#[derive(Clone, Debug)]
pub struct GraphSnapshot {
    pub events: HashMap<EventId, Event>,
    pub tips: Vec<EventId>,
    pub frontier: VectorClock,
}

impl From<&CausalGraph> for GraphSnapshot {
    fn from(graph: &CausalGraph) -> Self {
        Self {
            events: graph.events.clone(),
            tips: graph.tips.iter().copied().collect(),
            frontier: graph.frontier.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::vector_clock::VectorClock;

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn create_test_event(
        creator: NodeId,
        sequence: u64,
        self_parent: Option<EventId>,
        other_parent: Option<EventId>,
    ) -> Event {
        let mut vc = VectorClock::with_node(creator, sequence + 1);
        Event::new(creator, sequence, vc, self_parent, other_parent, vec![])
    }

    #[test]
    fn test_insert_and_retrieve() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let event = Event::genesis(n1, vec![1, 2, 3]);
        let id = event.id;
        event.validate().unwrap();
        let mut signed = event;
        signed.sign(vec![1, 2, 3]);

        graph.insert(signed.clone()).unwrap();
        assert!(graph.contains(&id));
        assert_eq!(graph.get(&id).unwrap().id, id);
    }

    #[test]
    fn test_duplicate_rejection() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut event = Event::genesis(n1, vec![]);
        event.sign(vec![1]);

        graph.insert(event.clone()).unwrap();
        assert!(matches!(
            graph.insert(event),
            Err(CausalGraphError::DuplicateEvent(_))
        ));
    }

    #[test]
    fn test_missing_parent() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let fake_parent = [99u8; 32];
        let event = Event::new(n1, 1, VectorClock::with_node(n1, 2), Some(fake_parent), None, vec![]);

        assert!(matches!(
            graph.insert(event),
            Err(CausalGraphError::MissingParent(_))
        ));
    }

    #[test]
    fn test_ancestry() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        // Genesis
        let mut g = Event::genesis(n1, vec![]);
        g.sign(vec![1]);
        let g_id = g.id;
        graph.insert(g).unwrap();

        // Child of genesis
        let vc = VectorClock::with_node(n1, 2);
        let mut child = Event::new(n1, 1, vc, Some(g_id), None, vec![]);
        child.sign(vec![1]);
        let child_id = child.id;
        graph.insert(child).unwrap();

        // Grandchild
        let vc = VectorClock::with_node(n1, 3);
        let mut gc = Event::new(n1, 2, vc, Some(child_id), None, vec![]);
        gc.sign(vec![1]);
        let gc_id = gc.id;
        graph.insert(gc).unwrap();

        assert!(graph.is_ancestor_of(&gc_id, &g_id).unwrap());
        assert!(graph.is_ancestor_of(&child_id, &g_id).unwrap());
        assert!(!graph.is_ancestor_of(&g_id, &child_id).unwrap());

        let ancestors = graph.get_ancestors(&gc_id).unwrap();
        assert!(ancestors.contains(&g_id));
        assert!(ancestors.contains(&child_id));
    }

    #[test]
    fn test_concurrent_events() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);
        let n2 = test_node(2);

        // n1 creates event
        let mut e1 = Event::genesis(n1, vec![1]);
        e1.sign(vec![1]);
        let e1_id = e1.id;
        graph.insert(e1).unwrap();

        // n2 creates event (different branch, concurrent)
        let mut e2 = Event::genesis(n2, vec![2]);
        e2.sign(vec![1]);
        let e2_id = e2.id;
        graph.insert(e2).unwrap();

        let concurrent = graph.find_concurrent(&e1_id);
        assert!(concurrent.iter().any(|e| e.id == e2_id));
    }

    #[test]
    fn test_tips_management() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        // First event is a tip
        let mut e1 = Event::genesis(n1, vec![]);
        e1.sign(vec![1]);
        let e1_id = e1.id;
        graph.insert(e1).unwrap();
        assert!(graph.tips().any(|&t| t == e1_id));

        // Second event references first, so first is no longer a tip
        let mut e2 = Event::new(
            n1,
            1,
            VectorClock::with_node(n1, 2),
            Some(e1_id),
            None,
            vec![],
        );
        e2.sign(vec![1]);
        let e2_id = e2.id;
        graph.insert(e2).unwrap();

        assert!(!graph.tips().any(|&t| t == e1_id));
        assert!(graph.tips().any(|&t| t == e2_id));
    }

    #[test]
    fn test_topological_order() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        // Chain: g -> a -> b
        let mut g = Event::genesis(n1, vec![]);
        g.sign(vec![1]);
        let g_id = g.id;
        graph.insert(g).unwrap();

        let mut a = Event::new(n1, 1, VectorClock::with_node(n1, 2), Some(g_id), None, vec![]);
        a.sign(vec![1]);
        let a_id = a.id;
        graph.insert(a).unwrap();

        let mut b = Event::new(n1, 2, VectorClock::with_node(n1, 3), Some(a_id), None, vec![]);
        b.sign(vec![1]);
        let b_id = b.id;
        graph.insert(b).unwrap();

        let order = graph.topological_order(None);
        let g_pos = order.iter().position(|&id| id == g_id).unwrap();
        let a_pos = order.iter().position(|&id| id == a_id).unwrap();
        let b_pos = order.iter().position(|&id| id == b_id).unwrap();

        assert!(g_pos < a_pos);
        assert!(a_pos < b_pos);
    }

    #[test]
    fn test_integrity_check() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut e = Event::genesis(n1, vec![]);
        e.sign(vec![1]);
        graph.insert(e).unwrap();

        assert!(graph.verify_integrity().is_ok());
    }

    #[test]
    fn test_stats() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);
        let n2 = test_node(2);

        let mut e1 = Event::genesis(n1, vec![]);
        e1.sign(vec![1]);
        graph.insert(e1).unwrap();

        let mut e2 = Event::genesis(n2, vec![]);
        e2.sign(vec![1]);
        graph.insert(e2).unwrap();

        let stats = graph.stats();
        assert_eq!(stats.total_events, 2);
        assert_eq!(stats.tip_count, 2);
        assert_eq!(stats.node_count, 2);
    }

    #[test]
    fn test_finalize() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut e = Event::genesis(n1, vec![]);
        e.sign(vec![1]);
        let e_id = e.id;
        graph.insert(e).unwrap();

        graph.finalize_event(&e_id).unwrap();
        assert_eq!(graph.get(&e_id).unwrap().status, EventStatus::Finalized);
        assert_eq!(graph.stats().finalized_events, 1);
    }
}
