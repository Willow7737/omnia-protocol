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

use crate::event::{Event, EventId, EventStatus};
use crate::vector_clock::{NodeId, VectorClock};
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
    /// Event already exists in the graph
    DuplicateEvent(String),
    #[error("Parent event {0} not found")]
    /// Parent event referenced but not found
    MissingParent(String),
    #[error("Cycle detected — cannot add event {0}")]
    /// Adding this event would create a cycle
    CycleDetected(String),
    #[error("Maximum ancestry depth exceeded starting from {0}")]
    /// Ancestry traversal exceeded maximum depth
    MaxDepthExceeded(String),
    #[error("Invalid event: {0}")]
    /// Event failed validation
    InvalidEvent(String),
    #[error("Graph integrity check failed: {0}")]
    /// Graph integrity violation
    IntegrityError(String),
    #[error("Event {0} has been pruned")]
    /// Event was pruned from the graph but minimal metadata is retained
    EventPruned(String),
}

/// Statistics about the causal graph
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStats {
    /// Total number of events in the graph
    pub total_events: usize,
    /// Number of current tip events
    pub tip_count: usize,
    /// Number of distinct creator nodes
    pub node_count: usize,
    /// Maximum depth of any event
    pub max_depth: usize,
    /// Number of finalized events
    pub finalized_events: usize,
    /// Number of pending events
    pub pending_events: usize,
}

/// Minimal metadata retained for pruned events.
///
/// When an event is pruned via [`CausalGraph::prune_finalized`], the full
/// event data is removed from the graph, but this struct preserves enough
/// information to maintain the DAG structure and respond to queries about
/// the event's existence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrunedEventMetadata {
    /// The event ID (hash).
    pub event_id: EventId,
    /// The creator of the event.
    pub creator: NodeId,
    /// The sequence number of the event.
    pub sequence: u64,
    /// The depth of the event in the graph.
    pub depth: usize,
    /// The round at which the event was finalized.
    pub finalized_round: u64,
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
    /// Per-event depth (computed during insert, used for pruning)
    depths: HashMap<EventId, usize>,
    /// Metadata for events that have been pruned.
    ///
    /// When events are pruned via [`prune_finalized()`], the full `Event`
    /// is removed from `events`, but a [`PrunedEventMetadata`] is stored
    /// here so that queries can distinguish between "never existed" and
    /// "pruned".
    pruned_events: HashMap<EventId, PrunedEventMetadata>,
    /// Per-event finalized round (set when `finalize_event` is called).
    finalized_rounds: HashMap<EventId, u64>,
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
            depths: HashMap::new(),
            pruned_events: HashMap::new(),
            finalized_rounds: HashMap::new(),
        }
    }

    /// Insert a new event into the graph
    pub fn insert(&mut self, event: Event) -> Result<(), CausalGraphError> {
        let event_id = event.id;

        // Check for duplicate
        if self.events.contains_key(&event_id) {
            return Err(CausalGraphError::DuplicateEvent(
                hex::encode(&event_id[..8]).to_string(),
            ));
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

        // Check for cycles
        if let Some(sp) = event.self_parent {
            if self.is_ancestor_of(&event_id, &sp)? {
                return Err(CausalGraphError::CycleDetected(
                    hex::encode(&event_id[..8]).to_string(),
                ));
            }
        }
        if let Some(op) = event.other_parent {
            if self.is_ancestor_of(&event_id, &op)? {
                return Err(CausalGraphError::CycleDetected(
                    hex::encode(&event_id[..8]).to_string(),
                ));
            }
        }

        // Remove parents from tips
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

        // Track finalized count
        if event.status == EventStatus::Finalized {
            self.finalized_count += 1;
        }

        // Store the event BEFORE calculating depth (depth calculation looks up events in the map)
        self.events.insert(event_id, event);

        // Update max depth
        let depth = self.calculate_depth(&event_id)?;
        self.max_depth = self.max_depth.max(depth);
        self.depths.insert(event_id, depth);

        // Prune tips if too many
        if self.tips.len() > MAX_TIPS {
            self.consolidate_tips();
        }

        Ok(())
    }

    /// Get an event by its ID.
    ///
    /// Returns `None` for both non-existent and pruned events.
    /// Use [`get_checked()`] to distinguish between these cases.
    pub fn get(&self, event_id: &EventId) -> Option<&Event> {
        self.events.get(event_id)
    }

    /// Get an event by its ID with full error discrimination.
    ///
    /// Unlike [`get()`], this method returns a `Result` that distinguishes
    /// between three cases:
    /// - `Ok(&Event)` — the event exists and is accessible
    /// - `Err(EventPruned)` — the event was pruned (minimal metadata retained)
    /// - `Err(InvalidEvent)` — the event was never in the graph
    pub fn get_checked(&self, event_id: &EventId) -> Result<&Event, CausalGraphError> {
        if let Some(event) = self.events.get(event_id) {
            Ok(event)
        } else if self.pruned_events.contains_key(event_id) {
            Err(CausalGraphError::EventPruned(hex::encode(&event_id[..8])))
        } else {
            Err(CausalGraphError::InvalidEvent(format!(
                "event not found: {}",
                hex::encode(&event_id[..8])
            )))
        }
    }

    /// Check whether an event has been pruned from the graph.
    ///
    /// Returns `true` if the event was previously in the graph but has
    /// been pruned via [`prune_finalized()`]. Returns `false` for events
    /// that are still present or never existed.
    pub fn is_pruned(&self, event_id: &EventId) -> bool {
        self.pruned_events.contains_key(event_id)
    }

    /// Get a mutable reference to an event
    pub fn get_mut(&mut self, event_id: &EventId) -> Option<&mut Event> {
        self.events.get_mut(event_id)
    }

    /// Check if an event exists in the graph
    pub fn contains(&self, event_id: &EventId) -> bool {
        self.events.contains_key(event_id)
    }

    /// Get all event IDs in the graph
    pub fn event_ids(&self) -> Vec<EventId> {
        self.events.keys().copied().collect()
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

    /// Check if `ancestor` is an ancestor of `descendant`
    pub fn is_ancestor_of(
        &self,
        descendant: &EventId,
        ancestor: &EventId,
    ) -> Result<bool, CausalGraphError> {
        if descendant == ancestor {
            return Ok(false);
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(*descendant);
        visited.insert(*descendant);

        let mut depth = 0;

        while let Some(current_id) = queue.pop_front() {
            depth += 1;
            if depth > MAX_ANCESTRY_DEPTH {
                return Err(CausalGraphError::MaxDepthExceeded(
                    hex::encode(&descendant[..8]).to_string(),
                ));
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

    /// Get all ancestors of an event
    pub fn get_ancestors(&self, event_id: &EventId) -> Result<HashSet<EventId>, CausalGraphError> {
        let mut ancestors = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(*event_id);

        let mut depth = 0;

        while let Some(current_id) = queue.pop_front() {
            depth += 1;
            if depth > MAX_ANCESTRY_DEPTH {
                return Err(CausalGraphError::MaxDepthExceeded(
                    hex::encode(&event_id[..8]).to_string(),
                ));
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

    /// Calculate the depth of an event
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

    /// Find events that are concurrent with the given event
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

    /// Get a topological ordering of events
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

        for event in &relevant_events {
            in_degree.entry(event.id).or_insert(0);
        }

        for event in &relevant_events {
            for parent in [event.self_parent, event.other_parent].iter().flatten() {
                if in_degree.contains_key(parent) {
                    children.entry(*parent).or_default().push(event.id);
                    *in_degree.entry(event.id).or_insert(0) += 1;
                }
            }
        }

        let mut queue: VecDeque<EventId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| *id)
            .collect();

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

    /// Find events that are in our graph but not in the given set
    pub fn diff(&self, known_events: &HashSet<EventId>) -> Vec<&Event> {
        self.events
            .values()
            .filter(|e| !known_events.contains(&e.id))
            .collect()
    }

    /// Find events newer than a given vector clock
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
                .filter(|e| e.status == EventStatus::Pending || e.status == EventStatus::Gossiped)
                .count(),
        }
    }

    /// Mark an event as finalized.
    ///
    /// Also records the finalized round for later use by
    /// [`prune_finalized()`].
    ///
    /// # Arguments
    ///
    /// * `event_id` — The ID of the event to finalize
    /// * `round` — The consensus round at which the event was finalized
    ///
    /// # Errors
    ///
    /// Returns [`CausalGraphError::InvalidEvent`] if the event is not found.
    /// Returns [`CausalGraphError::EventPruned`] if the event has been pruned.
    pub fn finalize_event_with_round(
        &mut self,
        event_id: &EventId,
        round: u64,
    ) -> Result<(), CausalGraphError> {
        if self.pruned_events.contains_key(event_id) {
            return Err(CausalGraphError::EventPruned(hex::encode(
                &event_id[..8],
            )));
        }
        if let Some(event) = self.events.get_mut(event_id) {
            if event.status != EventStatus::Finalized {
                event.status = EventStatus::Finalized;
                self.finalized_count += 1;
            }
            self.finalized_rounds.insert(*event_id, round);
            Ok(())
        } else {
            Err(CausalGraphError::InvalidEvent(format!(
                "event not found: {}",
                hex::encode(&event_id[..8])
            )))
        }
    }

    /// Mark an event as finalized (without tracking the round).
    ///
    /// This is the legacy version that does not record the finalized round.
    /// For proper pruning support, use [`finalize_event_with_round()`].
    pub fn finalize_event(&mut self, event_id: &EventId) -> Result<(), CausalGraphError> {
        if self.pruned_events.contains_key(event_id) {
            return Err(CausalGraphError::EventPruned(hex::encode(
                &event_id[..8],
            )));
        }
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

    /// Compute the Merkle root of all event hashes in the graph.
    ///
    /// This is the state commitment posted to Ethereum L1 by the ZK-rollup.
    /// The root changes whenever a new event is inserted, providing a
    /// cryptographic fingerprint of the entire L2 state.
    pub fn state_root(&self) -> [u8; 32] {
        let mut ids: Vec<&EventId> = self.events.keys().collect();
        if ids.is_empty() {
            return [0u8; 32];
        }

        // Sort for deterministic ordering
        ids.sort();

        // Build Merkle tree bottom-up
        let mut level: Vec<[u8; 32]> = ids
            .iter()
            .map(|id| blake3::hash(&**id).as_bytes().to_owned())
            .collect();

        while level.len() > 1 {
            let mut next_level = Vec::new();
            for chunk in level.chunks(2) {
                let mut hasher = blake3::Hasher::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(&chunk[1]);
                } else {
                    // Odd node out — hash with itself
                    hasher.update(&chunk[0]);
                }
                next_level.push(*hasher.finalize().as_bytes());
            }
            level = next_level;
        }

        level[0]
    }

    /// Verify that an event is included in the current state root.
    /// Returns the Merkle proof path (sibling hashes at each level).
    ///
    /// Used by ZK circuits to prove event inclusion on L1.
    pub fn merkle_proof(&self, event_id: &EventId) -> Option<Vec<([u8; 32], bool)>> {
        let mut ids: Vec<&EventId> = self.events.keys().collect();
        ids.sort();

        let pos = ids.iter().position(|&id| id == event_id)?;
        let mut proof = Vec::new();
        let mut index = pos;
        let mut level: Vec<[u8; 32]> = ids
            .iter()
            .map(|id| blake3::hash(&**id).as_bytes().to_owned())
            .collect();

        while level.len() > 1 {
            let sibling = if index % 2 == 0 {
                // We're the left child, sibling is right
                if index + 1 < level.len() {
                    (level[index + 1], true) // true = sibling is right
                } else {
                    (level[index], true) // Odd node, sibling is self
                }
            } else {
                // We're the right child, sibling is left
                (level[index - 1], false) // false = sibling is left
            };
            proof.push(sibling);
            index /= 2;

            let mut next_level = Vec::new();
            for chunk in level.chunks(2) {
                let mut hasher = blake3::Hasher::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                next_level.push(*hasher.finalize().as_bytes());
            }
            level = next_level;
        }

        Some(proof)
    }

    /// Remove payload data from events with depth less than `min_depth`.
    ///
    /// Preserves the event ID in the graph structure (for Merkle root
    /// computation) but removes the full Event data (payload) to save
    /// memory. This is called after events have been committed to L1
    /// and are no longer needed for consensus.
    pub fn prune_old_events(&mut self, min_depth: usize) {
        let to_prune: Vec<EventId> = self
            .depths
            .iter()
            .filter(|(_, &depth)| depth < min_depth)
            .map(|(id, _)| *id)
            .collect();

        for id in &to_prune {
            if let Some(event) = self.events.get_mut(id) {
                // Keep the event shell (for parent links and depth) but clear payload
                event.payload.clear();
            }
        }

        // Clean up tips that no longer exist
        self.tips.retain(|id| self.events.contains_key(id));
    }

    /// Prune finalized events older than a given depth from the current round.
    ///
    /// Removes fully finalized events whose `finalized_round` is before
    /// `current_round - depth`, keeping only minimal metadata in
    /// [`pruned_events`](Self::pruned_events). This reduces memory usage
    /// for long-running nodes while preserving enough information to
    /// distinguish "pruned" from "never existed".
    ///
    /// # Arguments
    ///
    /// * `current_round` — The current consensus round number
    /// * `depth` — Number of finalized rounds to retain. If `0`, this is
    ///   a no-op (archive mode: nothing is ever pruned).
    ///
    /// # Returns
    ///
    /// The number of events pruned in this call.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Keep the last 1000 rounds of finalized events
    /// let pruned = graph.prune_finalized(current_round, 1000);
    /// tracing::info!(pruned, "pruned old finalized events");
    /// ```
    pub fn prune_finalized(&mut self, current_round: u64, depth: u64) -> usize {
        // Archive mode: never prune
        if depth == 0 {
            return 0;
        }

        let cutoff_round = current_round.saturating_sub(depth);

        // Collect events that are finalized and old enough to prune
        let to_prune: Vec<EventId> = self
            .finalized_rounds
            .iter()
            .filter(|(_, &round)| round < cutoff_round)
            .map(|(id, _)| *id)
            .collect();

        let pruned_count = to_prune.len();

        for id in &to_prune {
            // Create minimal metadata from the event before removing it
            if let Some(event) = self.events.remove(id) {
                let finalized_round = self
                    .finalized_rounds
                    .remove(id)
                    .expect("finalized_rounds entry exists for event in to_prune");
                let depth_val = self.depths.remove(id).unwrap_or(0);

                let metadata = PrunedEventMetadata {
                    event_id: *id,
                    creator: event.creator,
                    sequence: event.sequence,
                    depth: depth_val,
                    finalized_round,
                };
                self.pruned_events.insert(*id, metadata);

                // Remove from tips if present
                self.tips.remove(id);
            }
        }

        // Remove pruned event IDs from by_creator index
        for id in &to_prune {
            let creator = if let Some(meta) = self.pruned_events.get(id) {
                meta.creator
            } else {
                continue;
            };
            if let Some(ids) = self.by_creator.get_mut(&creator) {
                ids.retain(|x| x != id);
            }
        }

        // Clean up empty by_creator entries
        self.by_creator.retain(|_, ids| !ids.is_empty());

        // Adjust finalized_count
        self.finalized_count = self.finalized_count.saturating_sub(pruned_count);

        if pruned_count > 0 {
            tracing::debug!(
                pruned_count,
                current_round,
                cutoff_round,
                "pruned finalized events"
            );
        }

        pruned_count
    }

    /// Get the total size of all payloads in bytes.
    pub fn payload_size(&self) -> usize {
        self.events.values().map(|e| e.payload.len()).sum()
    }

    /// Consolidate old tips
    fn consolidate_tips(&mut self) {
        if self.tips.len() <= MAX_TIPS {
            return;
        }

        let mut tip_events: Vec<&Event> = self
            .tips
            .iter()
            .filter_map(|id| self.events.get(id))
            .collect();

        tip_events.sort_by_key(|e| e.timestamp);

        let to_remove = self.tips.len() - MAX_TIPS + MAX_TIPS / 10;
        for event in tip_events.into_iter().take(to_remove) {
            self.tips.remove(&event.id);
        }
    }

    /// Verify graph integrity
    pub fn verify_integrity(&self) -> Result<(), CausalGraphError> {
        for (id, event) in &self.events {
            if let Some(sp) = event.self_parent {
                if !self.events.contains_key(&sp) {
                    return Err(CausalGraphError::IntegrityError(format!(
                        "event {} has dangling self-parent {}",
                        hex::encode(&id[..8]),
                        hex::encode(&sp[..8])
                    )));
                }
            }
            if let Some(op) = event.other_parent {
                if !self.events.contains_key(&op) {
                    return Err(CausalGraphError::IntegrityError(format!(
                        "event {} has dangling other-parent {}",
                        hex::encode(&id[..8]),
                        hex::encode(&op[..8])
                    )));
                }
            }
            if !event.verify_hash() {
                return Err(CausalGraphError::IntegrityError(format!(
                    "event {} has invalid hash",
                    hex::encode(&id[..8])
                )));
            }
        }

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

/// A read-only snapshot of the causal graph
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GraphSnapshot {
    /// All events in the snapshot
    pub events: HashMap<EventId, Event>,
    /// Current tip event IDs
    pub tips: Vec<EventId>,
    /// Frontier vector clock
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

    #[allow(dead_code)]
    fn create_test_event(
        creator: NodeId,
        sequence: u64,
        self_parent: Option<EventId>,
        other_parent: Option<EventId>,
    ) -> Event {
        let vc = VectorClock::with_node(creator, sequence + 1);
        Event::new(creator, sequence, vc, self_parent, other_parent, vec![])
    }

    #[test]
    fn test_insert_and_retrieve() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let event = Event::genesis(n1, vec![1, 2, 3]);
        let id = event.id;
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
        let event = Event::new(
            n1,
            1,
            VectorClock::with_node(n1, 2),
            Some(fake_parent),
            None,
            vec![],
        );

        assert!(matches!(
            graph.insert(event),
            Err(CausalGraphError::MissingParent(_))
        ));
    }

    #[test]
    fn test_ancestry() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut g = Event::genesis(n1, vec![]);
        g.sign(vec![1]);
        let g_id = g.id;
        graph.insert(g).unwrap();

        let vc = VectorClock::with_node(n1, 2);
        let mut child = Event::new(n1, 1, vc, Some(g_id), None, vec![]);
        child.sign(vec![1]);
        let child_id = child.id;
        graph.insert(child).unwrap();

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

        let mut e1 = Event::genesis(n1, vec![1]);
        e1.sign(vec![1]);
        let e1_id = e1.id;
        graph.insert(e1).unwrap();

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

        let mut e1 = Event::genesis(n1, vec![]);
        e1.sign(vec![1]);
        let e1_id = e1.id;
        graph.insert(e1).unwrap();
        assert!(graph.tips().any(|&t| t == e1_id));

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

        let mut g = Event::genesis(n1, vec![]);
        g.sign(vec![1]);
        let g_id = g.id;
        graph.insert(g).unwrap();

        let mut a = Event::new(
            n1,
            1,
            VectorClock::with_node(n1, 2),
            Some(g_id),
            None,
            vec![],
        );
        a.sign(vec![1]);
        let a_id = a.id;
        graph.insert(a).unwrap();

        let mut b = Event::new(
            n1,
            2,
            VectorClock::with_node(n1, 3),
            Some(a_id),
            None,
            vec![],
        );
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

    #[test]
    fn test_state_root_changes_on_insert() {
        let mut graph = CausalGraph::new();
        let root1 = graph.state_root();
        assert_eq!(root1, [0u8; 32]); // Empty graph

        let mut event = Event::genesis(test_node(1), vec![1, 2, 3]);
        event.sign(vec![1]);
        graph.insert(event).unwrap();

        let root2 = graph.state_root();
        assert_ne!(root1, root2); // Root changed after insert

        let mut event2 = Event::genesis(test_node(2), vec![4, 5, 6]);
        event2.sign(vec![1]);
        graph.insert(event2).unwrap();

        let root3 = graph.state_root();
        assert_ne!(root2, root3); // Root changed again
    }

    #[test]
    fn test_merkle_proof_verification() {
        let mut graph = CausalGraph::new();
        let mut event = Event::genesis(test_node(1), vec![1, 2, 3]);
        event.sign(vec![1]);
        let id = event.id;
        graph.insert(event).unwrap();

        let proof = graph.merkle_proof(&id).unwrap();
        let root = graph.state_root();

        // Verify proof manually
        let leaf = blake3::hash(&id).as_bytes().to_owned();
        let mut current = leaf;
        for (sibling, sibling_is_right) in proof {
            let mut hasher = blake3::Hasher::new();
            if sibling_is_right {
                hasher.update(&current);
                hasher.update(&sibling);
            } else {
                hasher.update(&sibling);
                hasher.update(&current);
            }
            current = *hasher.finalize().as_bytes();
        }

        assert_eq!(current, root);
    }

    #[test]
    fn test_prune_old_events() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        // Create a chain of events with increasing depth
        let mut prev_id = None;
        for i in 0..5u8 {
            let mut event = if let Some(pid) = prev_id {
                Event::new(
                    n1,
                    i as u64,
                    VectorClock::with_node(n1, (i + 1) as u64),
                    Some(pid),
                    None,
                    vec![i],
                )
            } else {
                Event::genesis(n1, vec![i])
            };
            event.sign(vec![1]);
            let id = event.id;
            graph.insert(event).unwrap();
            prev_id = Some(id);
        }

        assert_eq!(graph.len(), 5);
        let size_before = graph.payload_size();
        assert!(size_before > 0);

        // Prune events with depth < 3 (events at depth 1 and 2)
        graph.prune_old_events(3);

        // Events with depth >= 3 still have payloads
        // Events with depth < 3 have empty payloads
        let size_after = graph.payload_size();
        assert!(size_after < size_before);
        assert!(size_after > 0); // Depth 3, 4, 5 still have payloads

        // Graph structure is preserved
        assert_eq!(graph.len(), 5);
    }

    // ── Merkle Proof Hardening Tests (Sprint 1, Task 2.4) ───────────

    /// Helper: build a chain of events with distinct payloads and return their IDs.
    fn build_chain(graph: &mut CausalGraph, node: NodeId, count: usize) -> Vec<EventId> {
        let mut ids = Vec::new();
        let mut prev_id = None;
        for i in 0..count {
            let payload = vec![i as u8; 8]; // 8-byte distinct payload per event
            let mut event = if let Some(pid) = prev_id {
                Event::new(
                    node,
                    i as u64,
                    VectorClock::with_node(node, (i + 1) as u64),
                    Some(pid),
                    None,
                    payload,
                )
            } else {
                Event::genesis(node, payload)
            };
            event.sign(vec![1]);
            let id = event.id;
            graph.insert(event).unwrap();
            prev_id = Some(id);
            ids.push(id);
        }
        ids
    }

    /// Helper: verify a Merkle proof against a known root.
    fn verify_merkle_proof(
        event_id: &EventId,
        proof: &[([u8; 32], bool)],
        root: &[u8; 32],
    ) -> bool {
        let leaf = blake3::hash(event_id).as_bytes().to_owned();
        let mut current = leaf;
        for (sibling, sibling_is_right) in proof {
            let mut hasher = blake3::Hasher::new();
            if *sibling_is_right {
                hasher.update(&current);
                hasher.update(sibling);
            } else {
                hasher.update(sibling);
                hasher.update(&current);
            }
            current = *hasher.finalize().as_bytes();
        }
        current == *root
    }

    /// Test that state_root() remains identical before and after pruning.
    ///
    /// Pruning only clears event payloads, not event IDs. Since the Merkle tree
    /// is built over event IDs (not payloads), the root must not change.
    #[test]
    fn test_state_root_unchanged_after_pruning() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let ids = build_chain(&mut graph, n1, 7);
        assert_eq!(graph.len(), 7);

        // Record root before pruning
        let root_before = graph.state_root();
        assert_ne!(root_before, [0u8; 32]); // Non-trivial graph

        // Prune events with depth < 4 (prunes depths 1, 2, 3)
        graph.prune_old_events(4);

        // Root must be identical after pruning
        let root_after = graph.state_root();
        assert_eq!(
            root_before, root_after,
            "state_root changed after pruning — Merkle tree must be built over event IDs only"
        );

        // Verify some payloads were actually cleared
        let _size_before_prune: usize = ids
            .iter()
            .map(|id| graph.get(id).unwrap().payload.len())
            .sum();
        // Some events should have empty payloads (pruned) and some shouldn't
        let pruned_count = ids
            .iter()
            .filter(|id| graph.get(id).unwrap().payload.is_empty())
            .count();
        assert!(
            pruned_count > 0,
            "No events were pruned — test is ineffective"
        );
        assert!(
            pruned_count < ids.len(),
            "All events were pruned — test is ineffective"
        );
    }

    /// Test that merkle_proof() still produces valid proofs for events
    /// that were NOT pruned (i.e., events whose payloads are intact).
    #[test]
    fn test_merkle_proof_valid_after_pruning_for_unpruned_events() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let ids = build_chain(&mut graph, n1, 7);
        assert_eq!(graph.len(), 7);

        // Prune events with depth < 4 (prunes depths 1, 2, 3)
        graph.prune_old_events(4);

        let root = graph.state_root();

        // Find events that were NOT pruned (payload still present)
        let unpruned_ids: Vec<EventId> = ids
            .iter()
            .filter(|id| !graph.get(id).unwrap().payload.is_empty())
            .copied()
            .collect();

        assert!(!unpruned_ids.is_empty(), "No unpruned events to test");

        // Verify Merkle proofs for all unpruned events
        for id in &unpruned_ids {
            let proof = graph
                .merkle_proof(id)
                .expect("merkle_proof should return Some for existing event");
            assert!(
                verify_merkle_proof(id, &proof, &root),
                "Merkle proof failed for unpruned event after pruning"
            );
        }
    }

    /// Test that pruned events (with empty payloads) still have valid
    /// Merkle proofs for their existence in the graph.
    ///
    /// This is critical for L1 verification: even after old events are
    /// pruned (payloads cleared to save memory), their existence in the
    /// DAG must still be provable via Merkle inclusion proofs.
    #[test]
    fn test_merkle_proof_valid_for_pruned_events() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let ids = build_chain(&mut graph, n1, 7);
        assert_eq!(graph.len(), 7);

        // Record root BEFORE pruning (root must not change)
        let root_before = graph.state_root();

        // Prune events with depth < 4 (prunes depths 1, 2, 3)
        graph.prune_old_events(4);

        // Root must be unchanged
        let root_after = graph.state_root();
        assert_eq!(root_before, root_after);

        // Find events that WERE pruned (payload cleared)
        let pruned_ids: Vec<EventId> = ids
            .iter()
            .filter(|id| graph.get(id).unwrap().payload.is_empty())
            .copied()
            .collect();

        assert!(!pruned_ids.is_empty(), "No pruned events to test");

        // Verify Merkle proofs still work for pruned events
        for id in &pruned_ids {
            let proof = graph
                .merkle_proof(id)
                .expect("merkle_proof should return Some for pruned event still in graph");
            assert!(
                verify_merkle_proof(id, &proof, &root_after),
                "Merkle proof failed for pruned event — existence must still be provable"
            );
        }
    }

    // ── Event Pruning Tests (Sprint 4, Task B4) ──────────────────────

    /// Test that prune_finalized returns 0 when depth is 0 (archive mode).
    #[test]
    fn test_prune_finalized_archive_mode() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut e = Event::genesis(n1, vec![1, 2, 3]);
        e.sign(vec![1]);
        let e_id = e.id;
        graph.insert(e).unwrap();
        graph.finalize_event_with_round(&e_id, 1).unwrap();

        // depth=0 means archive mode — nothing should be pruned
        let pruned = graph.prune_finalized(100, 0);
        assert_eq!(pruned, 0);
        assert!(graph.contains(&e_id));
        assert!(!graph.is_pruned(&e_id));
    }

    /// Test that prune_finalized removes old finalized events.
    #[test]
    fn test_prune_finalized_basic() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        // Create two events
        let mut e1 = Event::genesis(n1, vec![1]);
        e1.sign(vec![1]);
        let e1_id = e1.id;
        graph.insert(e1).unwrap();

        let mut e2 = Event::new(
            n1,
            1,
            VectorClock::with_node(n1, 2),
            Some(e1_id),
            None,
            vec![2],
        );
        e2.sign(vec![1]);
        let e2_id = e2.id;
        graph.insert(e2).unwrap();

        // Finalize both at different rounds
        graph.finalize_event_with_round(&e1_id, 1).unwrap();
        graph.finalize_event_with_round(&e2_id, 5).unwrap();

        // Prune with depth=3 from round 5: cutoff = 5-3 = 2
        // e1 was finalized at round 1 < 2, so it should be pruned
        // e2 was finalized at round 5 >= 2, so it should remain
        let pruned = graph.prune_finalized(5, 3);
        assert_eq!(pruned, 1);
        assert!(!graph.contains(&e1_id));
        assert!(graph.is_pruned(&e1_id));
        assert!(graph.contains(&e2_id));
        assert!(!graph.is_pruned(&e2_id));
    }

    /// Test that get_checked distinguishes pruned vs non-existent events.
    #[test]
    fn test_get_checked_pruned_vs_not_found() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut e = Event::genesis(n1, vec![1]);
        e.sign(vec![1]);
        let e_id = e.id;
        graph.insert(e).unwrap();
        graph.finalize_event_with_round(&e_id, 1).unwrap();

        // Before pruning: event is accessible
        assert!(graph.get_checked(&e_id).is_ok());

        // Prune the event
        graph.prune_finalized(10, 5);

        // After pruning: get_checked returns EventPruned error
        let result = graph.get_checked(&e_id);
        assert!(matches!(result, Err(CausalGraphError::EventPruned(_))));

        // A never-existent event returns InvalidEvent
        let fake_id = [99u8; 32];
        let result = graph.get_checked(&fake_id);
        assert!(matches!(result, Err(CausalGraphError::InvalidEvent(_))));
    }

    /// Test that finalize_event_with_round rejects pruned events.
    #[test]
    fn test_finalize_rejects_pruned() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut e = Event::genesis(n1, vec![1]);
        e.sign(vec![1]);
        let e_id = e.id;
        graph.insert(e).unwrap();
        graph.finalize_event_with_round(&e_id, 1).unwrap();

        // Prune the event
        graph.prune_finalized(10, 5);

        // Attempting to finalize a pruned event should fail
        let result = graph.finalize_event_with_round(&e_id, 99);
        assert!(matches!(result, Err(CausalGraphError::EventPruned(_))));
    }

    /// Test that pruned event metadata is preserved correctly.
    #[test]
    fn test_pruned_metadata_preserved() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut e = Event::genesis(n1, vec![42]);
        e.sign(vec![1]);
        let e_id = e.id;
        graph.insert(e).unwrap();
        graph.finalize_event_with_round(&e_id, 7).unwrap();

        // Prune the event
        let pruned = graph.prune_finalized(20, 10);
        assert_eq!(pruned, 1);

        // Verify metadata is in pruned_events (we can't access the field directly,
        // but is_pruned should return true)
        assert!(graph.is_pruned(&e_id));
        assert!(!graph.contains(&e_id));

        // The event should no longer be in by_creator
        let by_creator = graph.by_creator(&n1);
        assert!(by_creator.is_empty());
    }

    /// Test that prune_finalized with no qualifying events returns 0.
    #[test]
    fn test_prune_finalized_nothing_to_prune() {
        let mut graph = CausalGraph::new();
        let n1 = test_node(1);

        let mut e = Event::genesis(n1, vec![1]);
        e.sign(vec![1]);
        let e_id = e.id;
        graph.insert(e).unwrap();
        graph.finalize_event_with_round(&e_id, 10).unwrap();

        // Prune with a cutoff that doesn't qualify any events
        // current_round=5, depth=3 -> cutoff=2. Event at round 10 is NOT pruned.
        let pruned = graph.prune_finalized(5, 3);
        assert_eq!(pruned, 0);
        assert!(graph.contains(&e_id));
    }

    /// Test that SubstrateConfig defaults to archive mode (pruning_depth=0).
    #[test]
    fn test_substrate_config_default_pruning_depth() {
        use crate::SubstrateConfig;
        let config = SubstrateConfig::new(test_node(1));
        assert_eq!(config.pruning_depth, 0, "Default pruning_depth should be 0 (archive mode)");
    }
}
