//! Pruning-aware event pool
//!
//! Extends [`EventPool`] with pruning support: events marked as "finalized"
//! can have their slots freed for reuse while maintaining correctness.
//! This prevents memory bloat under steady-state operation.
//!
//! # Architecture
//!
//! `PruningAwarePool` combines three components:
//!
//! 1. **EventPool** — Pre-allocated arena for event storage (slab allocator)
//! 2. **VectorClockIndex** — O(1) parent resolution via (creator, sequence) index
//! 3. **Pruned event metadata** — Minimal metadata retained for pruned events
//!
//! When an event is finalized and then pruned, its slot in the EventPool
//! is freed for reuse, but enough metadata is retained to distinguish
//! "pruned" from "never existed" — matching the semantics of
//! [`crate::causal_graph::CausalGraph::prune_finalized()`].

use std::collections::{HashMap, VecDeque};

use omnia_primitives::{Event, EventId, EventStatus, NodeId};

use crate::causal_graph::{CausalGraphError, PrunedEventMetadata};
use crate::event_pool::{EventPool, EventPoolError};
use crate::vector_clock_index::VectorClockIndex;

/// Maximum number of pruned event metadata entries to retain.
/// When exceeded, the oldest entries are evicted.
const MAX_PRUNED_EVENTS: usize = 50_000;

/// Comprehensive statistics about the pruning-aware pool.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolStats {
    /// Total number of occupied slots in the event pool.
    pub occupied_slots: usize,
    /// Total capacity of the event pool.
    pub pool_capacity: usize,
    /// Number of free slots available for reuse.
    pub free_slots: usize,
    /// Number of distinct creators tracked in the vector clock index.
    pub indexed_creators: usize,
    /// Number of (creator, sequence) entries in the vector clock index.
    pub index_entries: usize,
    /// Number of events that have been finalized.
    pub finalized_count: usize,
    /// Number of events that have been pruned.
    pub pruned_count: usize,
    /// Pool utilization ratio (occupied / capacity).
    pub utilization: f64,
    /// Number of times the pool has grown.
    pub growth_count: usize,
}

/// Pruning-aware event pool that combines pre-allocated storage with
/// O(1) vector clock indexing and slot reuse from finalized event pruning.
///
/// This is the main entry point for the optimized DAG insertion path.
/// It integrates `EventPool` (slab allocator), `VectorClockIndex`
/// (parent resolution), and pruning metadata (correctness) into a
/// single cohesive data structure.
///
/// # Lifecycle
///
/// 1. **Insert** — Event is placed in the `EventPool` and indexed in the
///    `VectorClockIndex`. No heap allocation occurs (slot is reused from
///    free list or pre-allocated).
/// 2. **Finalize** — Event is marked as finalized. The slot is still
///    occupied, but the event is now eligible for pruning.
/// 3. **Prune** — Finalized events older than a given depth are removed
///    from the `EventPool` (freeing their slots) and their minimal
///    metadata is retained in the `pruned_metadata` map.
///
/// # Example
///
/// ```ignore
/// use omnia_consensus::pruning_aware_pool::PruningAwarePool;
/// let pool = PruningAwarePool::new(1024, 1_000_000);
/// let slot = pool.insert(event)?;
/// pool.mark_finalized(&event_id, round)?;
/// pool.prune_finalized(current_round, 1000);
/// ```
pub struct PruningAwarePool {
    /// Inner event pool (slab allocator).
    pool: EventPool,
    /// Vector clock index for O(1) parent lookups.
    index: VectorClockIndex,
    /// Pruned event metadata (same as CausalGraph's pruned_events).
    pruned_metadata: HashMap<EventId, PrunedEventMetadata>,
    /// Pruned event order for eviction (FIFO).
    pruned_order: VecDeque<EventId>,
    /// Per-event finalized round tracking.
    finalized_rounds: HashMap<EventId, u64>,
    /// Number of finalized events currently in the pool.
    finalized_count: usize,
}

impl PruningAwarePool {
    /// Create a new pruning-aware pool with the given initial and max capacities.
    ///
    /// Pre-allocates `initial_capacity` slots in the inner `EventPool`.
    ///
    /// # Panics
    ///
    /// Panics if `initial_capacity` is 0 or greater than `max_capacity`.
    pub fn new(initial_capacity: usize, max_capacity: usize) -> Self {
        Self {
            pool: EventPool::new(initial_capacity, max_capacity),
            index: VectorClockIndex::with_capacity(64, initial_capacity),
            pruned_metadata: HashMap::new(),
            pruned_order: VecDeque::new(),
            finalized_rounds: HashMap::new(),
            finalized_count: 0,
        }
    }

    /// Insert an event into the pool with vector clock indexing.
    ///
    /// The event is stored in the pre-allocated pool and indexed by
    /// (creator, sequence) for O(1) parent resolution.
    ///
    /// # Errors
    ///
    /// - [`EventPoolError::DuplicateEvent`] — an event with the same ID exists.
    /// - [`EventPoolError::PoolFull`] — pool is at max capacity.
    pub fn insert(&mut self, event: Event) -> Result<usize, EventPoolError> {
        let event_id = event.id;
        let creator = event.creator;
        let sequence = event.sequence;

        let slot = self.pool.insert(event)?;

        // Index the event for O(1) parent resolution
        self.index
            .index_event(&creator, sequence, slot, event_id)
            .map_err(|e| {
                // If indexing fails (e.g., sequence too large), remove the event from the pool
                // to maintain consistency, then convert to an appropriate error.
                self.pool.remove(&event_id);
                EventPoolError::InvalidEvent(format!("indexing failed: {}", e))
            })?;

        Ok(slot)
    }

    /// Get an event by its ID, with pruned-event error discrimination.
    ///
    /// Returns `Ok(&Event)` if the event is in the pool.
    /// Returns `Err(CausalGraphError::EventPruned)` if the event was pruned.
    /// Returns `Err(CausalGraphError::InvalidEvent)` if the event never existed.
    pub fn get(&self, event_id: &EventId) -> Result<&Event, CausalGraphError> {
        if let Some(event) = self.pool.get(event_id) {
            Ok(event)
        } else if self.pruned_metadata.contains_key(event_id) {
            Err(CausalGraphError::EventPruned(hex::encode(&event_id[..8])))
        } else {
            Err(CausalGraphError::InvalidEvent(format!(
                "event not found: {}",
                hex::encode(&event_id[..8])
            )))
        }
    }

    /// Get a mutable reference to an event by its ID.
    ///
    /// Returns `None` if the event is not in the pool (including if pruned).
    pub fn get_mut(&mut self, event_id: &EventId) -> Option<&mut Event> {
        self.pool.get_mut(event_id)
    }

    /// Get an event by its ID without error discrimination.
    ///
    /// Returns `None` for both non-existent and pruned events.
    pub fn get_opt(&self, event_id: &EventId) -> Option<&Event> {
        self.pool.get(event_id)
    }

    /// Resolve a parent event by creator and sequence number.
    ///
    /// Uses the pre-computed vector clock index for O(1) resolution.
    /// Returns `Some(&Event)` if found, `None` otherwise.
    pub fn resolve_parent(&self, creator: &NodeId, sequence: u64) -> Option<&Event> {
        let (_event_id, slot) = self.index.resolve_parent(creator, sequence)?;
        self.pool.get_by_slot(slot)
    }

    /// Resolve a parent event with full error discrimination.
    ///
    /// Like [`Self::resolve_parent()`], but returns a `Result` that
    /// distinguishes between "found", "pruned", and "not found".
    pub fn resolve_parent_checked(&self, creator: &NodeId, sequence: u64) -> Result<&Event, CausalGraphError> {
        if let Some((_event_id, slot)) = self.index.resolve_parent(creator, sequence) {
            if let Some(event) = self.pool.get_by_slot(slot) {
                return Ok(event);
            }
        }

        // Check if the event was pruned — we need to look through pruned_metadata
        // to find an event with matching creator and sequence
        for meta in self.pruned_metadata.values() {
            if meta.creator == *creator && meta.sequence == sequence {
                return Err(CausalGraphError::EventPruned(hex::encode(&meta.event_id[..8])));
            }
        }

        Err(CausalGraphError::InvalidEvent(format!(
            "parent not found: creator={}/seq={}",
            hex::encode(&creator[..4]),
            sequence
        )))
    }

    /// Check if an event exists in the pool (not pruned, not absent).
    pub fn contains(&self, event_id: &EventId) -> bool {
        self.pool.contains(event_id)
    }

    /// Mark an event as finalized with the given round number.
    ///
    /// The event must currently be in the pool (not pruned).
    ///
    /// # Errors
    ///
    /// - [`CausalGraphError::EventPruned`] — the event has already been pruned.
    /// - [`CausalGraphError::InvalidEvent`] — the event was never in the pool.
    pub fn mark_finalized(&mut self, event_id: &EventId, round: u64) -> Result<(), CausalGraphError> {
        if self.pruned_metadata.contains_key(event_id) {
            return Err(CausalGraphError::EventPruned(hex::encode(&event_id[..8])));
        }

        if let Some(event) = self.pool.get_mut(event_id) {
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

    /// Prune finalized events that are older than `current_round - depth`.
    ///
    /// Removes fully finalized events whose `finalized_round` is before
    /// `current_round - depth`, freeing their slots for reuse. Minimal
    /// metadata is retained so that queries can distinguish "pruned"
    /// from "never existed".
    ///
    /// # Arguments
    ///
    /// * `current_round` — The current consensus round number.
    /// * `depth` — Number of finalized rounds to retain. If `0`, this
    ///   is a no-op (archive mode: nothing is ever pruned).
    ///
    /// # Returns
    ///
    /// The number of events pruned in this call.
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
            // Get the finalized round
            let finalized_round = match self.finalized_rounds.remove(id) {
                Some(r) => r,
                None => continue,
            };

            // Remove the event from the pool (freeing its slot)
            if let Some(event) = self.pool.remove(id) {
                // Remove from vector clock index
                self.index.remove_by_creator_sequence(&event.creator, event.sequence);

                // Create minimal metadata
                let metadata = PrunedEventMetadata {
                    event_id: *id,
                    creator: event.creator,
                    sequence: event.sequence,
                    depth: 0, // We don't track depth in the pool; the CausalGraph does
                    finalized_round,
                    content_hash: event.content_hash(),
                };
                self.pruned_metadata.insert(*id, metadata);
                self.pruned_order.push_back(*id);
            }
        }

        // Adjust finalized count
        self.finalized_count = self.finalized_count.saturating_sub(pruned_count);

        // Evict oldest pruned_metadata entries if we exceeded the bound
        self.evict_pruned_events();

        if pruned_count > 0 {
            tracing::debug!(
                pruned_count,
                current_round,
                cutoff_round,
                "pruned finalized events from pool"
            );
        }

        pruned_count
    }

    /// Check whether an event has been pruned from the pool.
    pub fn is_pruned(&self, event_id: &EventId) -> bool {
        self.pruned_metadata.contains_key(event_id)
    }

    /// Get the pruned metadata for an event, if it has been pruned.
    pub fn get_pruned_metadata(&self, event_id: &EventId) -> Option<&PrunedEventMetadata> {
        self.pruned_metadata.get(event_id)
    }

    /// Get the number of events currently in the pool.
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    /// Check if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }

    /// Get the total capacity of the pool.
    pub fn capacity(&self) -> usize {
        self.pool.capacity()
    }

    /// Get the number of free slots in the pool.
    pub fn free_count(&self) -> usize {
        self.pool.free_count()
    }

    /// Check if the pool is at steady state (no growth needed).
    pub fn is_at_steady_state(&self) -> bool {
        self.pool.is_at_steady_state()
    }

    /// Get comprehensive statistics about the pool.
    pub fn stats(&self) -> PoolStats {
        let pool_stats = self.pool.stats();
        let index_stats = self.index.stats();

        let utilization = if pool_stats.total_capacity > 0 {
            pool_stats.occupied as f64 / pool_stats.total_capacity as f64
        } else {
            0.0
        };

        PoolStats {
            occupied_slots: pool_stats.occupied,
            pool_capacity: pool_stats.total_capacity,
            free_slots: pool_stats.free,
            indexed_creators: index_stats.creator_count,
            index_entries: index_stats.total_entries,
            finalized_count: self.finalized_count,
            pruned_count: self.pruned_metadata.len(),
            utilization,
            growth_count: pool_stats.growth_count,
        }
    }

    /// Evict the oldest pruned event metadata entries when the collection
    /// exceeds [`MAX_PRUNED_EVENTS`]. This prevents unbounded memory growth.
    fn evict_pruned_events(&mut self) {
        while self.pruned_metadata.len() > MAX_PRUNED_EVENTS {
            if let Some(oldest_id) = self.pruned_order.pop_front() {
                self.pruned_metadata.remove(&oldest_id);
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_primitives::{Event, VectorClock};

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn make_event(creator: NodeId, seq: u64) -> Event {
        use std::sync::atomic::{AtomicU64, Ordering};
        static EVENT_COUNTER: AtomicU64 = AtomicU64::new(0);
        let vc = VectorClock::with_node(creator, seq + 1);
        let counter = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let payload = counter.to_le_bytes().to_vec();
        Event::new(creator, seq, vc, None, None, payload).expect("valid event")
    }

    #[test]
    fn test_insert_and_get() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);
        let event = make_event(creator, 0);
        let event_id = event.id;

        pool.insert(event).expect("insert should succeed");
        assert_eq!(pool.len(), 1);

        let retrieved = pool.get(&event_id).expect("get should succeed");
        assert_eq!(retrieved.creator, creator);
    }

    #[test]
    fn test_get_pruned_event() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        // Insert and finalize event
        let event = make_event(creator, 0);
        let event_id = event.id;
        pool.insert(event).expect("insert should succeed");
        pool.mark_finalized(&event_id, 1).expect("finalize should succeed");

        // Prune the event
        let pruned = pool.prune_finalized(100, 10);
        assert_eq!(pruned, 1);

        // Event should now be pruned
        assert!(pool.is_pruned(&event_id));
        assert!(pool.get(&event_id).is_err());

        let result = pool.get(&event_id);
        assert!(matches!(result, Err(CausalGraphError::EventPruned(_))));
    }

    #[test]
    fn test_resolve_parent() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        let event = make_event(creator, 5);
        pool.insert(event).expect("insert should succeed");

        let parent = pool.resolve_parent(&creator, 5);
        assert!(parent.is_some());
        assert_eq!(parent.expect("parent").creator, creator);
        assert_eq!(parent.expect("parent").sequence, 5);
    }

    #[test]
    fn test_resolve_parent_checked() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        // Not found
        let result = pool.resolve_parent_checked(&creator, 0);
        assert!(matches!(result, Err(CausalGraphError::InvalidEvent(_))));

        // Insert and find
        let event = make_event(creator, 0);
        pool.insert(event).expect("insert should succeed");

        let result = pool.resolve_parent_checked(&creator, 0);
        assert!(result.is_ok());

        // Insert, finalize, prune, then resolve should return EventPruned
        let event2 = make_event(creator, 1);
        let event2_id = event2.id;
        pool.insert(event2).expect("insert should succeed");
        pool.mark_finalized(&event2_id, 1).expect("finalize should succeed");
        pool.prune_finalized(100, 10);

        let result = pool.resolve_parent_checked(&creator, 1);
        assert!(matches!(result, Err(CausalGraphError::EventPruned(_))));
    }

    #[test]
    fn test_mark_finalized() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        let event = make_event(creator, 0);
        let event_id = event.id;
        pool.insert(event).expect("insert should succeed");

        pool.mark_finalized(&event_id, 5).expect("finalize should succeed");

        let retrieved = pool.get(&event_id).expect("get should succeed");
        assert_eq!(retrieved.status, EventStatus::Finalized);

        let stats = pool.stats();
        assert_eq!(stats.finalized_count, 1);
    }

    #[test]
    fn test_mark_finalized_pruned_event() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        let event = make_event(creator, 0);
        let event_id = event.id;
        pool.insert(event).expect("insert should succeed");
        pool.mark_finalized(&event_id, 1).expect("finalize should succeed");
        pool.prune_finalized(100, 10);

        // Trying to finalize a pruned event should fail
        let result = pool.mark_finalized(&event_id, 2);
        assert!(matches!(result, Err(CausalGraphError::EventPruned(_))));
    }

    #[test]
    fn test_prune_finalized_archive_mode() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        let event = make_event(creator, 0);
        let event_id = event.id;
        pool.insert(event).expect("insert should succeed");
        pool.mark_finalized(&event_id, 1).expect("finalize should succeed");

        // depth=0 means archive mode, nothing pruned
        let pruned = pool.prune_finalized(100, 0);
        assert_eq!(pruned, 0);
        assert!(!pool.is_pruned(&event_id));
    }

    #[test]
    fn test_prune_finalized_selective() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        // Insert and finalize two events at different rounds
        let event1 = make_event(creator, 0);
        let event1_id = event1.id;
        pool.insert(event1).expect("insert should succeed");
        pool.mark_finalized(&event1_id, 5).expect("finalize should succeed");

        let event2 = make_event(test_node(2), 0);
        let event2_id = event2.id;
        pool.insert(event2).expect("insert should succeed");
        pool.mark_finalized(&event2_id, 50).expect("finalize should succeed");

        // Prune with depth=20 from round 50: cutoff=30, event1 (round 5) is pruned
        let pruned = pool.prune_finalized(50, 20);
        assert_eq!(pruned, 1);
        assert!(pool.is_pruned(&event1_id));
        assert!(!pool.is_pruned(&event2_id));

        // event2 should still be accessible
        let result = pool.get(&event2_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_slot_reuse_after_pruning() {
        let mut pool = PruningAwarePool::new(4, 100);
        let creator = test_node(1);

        // Fill pool
        for i in 0..4 {
            let event = make_event(creator, i);
            pool.insert(event).expect("insert should succeed");
        }
        assert_eq!(pool.free_count(), 0);

        // Finalize and prune two events
        // We need to get the event IDs to finalize them
        // Since we can't iterate the pool directly, let's re-create with known IDs
        pool = PruningAwarePool::new(4, 100);

        let event1 = make_event(creator, 0);
        let event1_id = event1.id;
        pool.insert(event1).expect("insert should succeed");

        let event2 = make_event(test_node(2), 0);
        let event2_id = event2.id;
        pool.insert(event2).expect("insert should succeed");

        pool.mark_finalized(&event1_id, 1).expect("finalize should succeed");
        pool.mark_finalized(&event2_id, 1).expect("finalize should succeed");

        let pruned = pool.prune_finalized(100, 10);
        assert_eq!(pruned, 2);

        // Slots should be free now
        assert!(pool.free_count() >= 2);

        // New inserts should use freed slots without growth
        let event3 = make_event(test_node(3), 0);
        pool.insert(event3).expect("insert should reuse free slot");
    }

    #[test]
    fn test_get_pruned_metadata() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        let event = make_event(creator, 42);
        let event_id = event.id;
        pool.insert(event).expect("insert should succeed");
        pool.mark_finalized(&event_id, 10).expect("finalize should succeed");
        pool.prune_finalized(100, 10);

        let metadata = pool.get_pruned_metadata(&event_id);
        assert!(metadata.is_some());
        let meta = metadata.expect("metadata should exist");
        assert_eq!(meta.creator, creator);
        assert_eq!(meta.sequence, 42);
        assert_eq!(meta.finalized_round, 10);
    }

    #[test]
    fn test_stats() {
        let mut pool = PruningAwarePool::new(64, 10_000);
        let creator = test_node(1);

        for i in 0..5 {
            let event = make_event(creator, i);
            pool.insert(event).expect("insert should succeed");
        }

        let stats = pool.stats();
        assert_eq!(stats.occupied_slots, 5);
        assert_eq!(stats.pool_capacity, 64);
        assert_eq!(stats.free_slots, 59);
        assert!(stats.indexed_creators >= 1);
        assert_eq!(stats.index_entries, 5);
        assert_eq!(stats.finalized_count, 0);
        assert_eq!(stats.pruned_count, 0);
    }

    #[test]
    fn test_eviction_of_pruned_metadata() {
        let mut pool = PruningAwarePool::new(1024, 100_000);

        // Insert, finalize, and prune more events than MAX_PRUNED_EVENTS
        for i in 0..(MAX_PRUNED_EVENTS + 100) {
            let creator = test_node(((i % 100) + 1) as u8);
            let event = make_event(creator, (i / 100) as u64);
            let event_id = event.id;
            pool.insert(event).expect("insert should succeed");
            pool.mark_finalized(&event_id, 1).expect("finalize should succeed");
        }

        // Prune all
        pool.prune_finalized(100, 10);

        // Pruned count should be capped at MAX_PRUNED_EVENTS
        assert!(pool.pruned_metadata.len() <= MAX_PRUNED_EVENTS);
    }
}
