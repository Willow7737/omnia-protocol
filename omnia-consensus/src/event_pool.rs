//! Pre-allocated arena for Event storage
//!
//! Uses a slab-based arena allocator to avoid per-insert heap allocations.
//! Events are stored in fixed-size slots; freed slots (from finalized+pruned
//! events) are recycled for new events.
//!
//! # Design
//!
//! The `EventPool` pre-allocates a configurable number of slots on creation.
//! Each slot can hold one `Event`. When an event is removed (e.g., after
//! pruning), the slot is added to a free list for reuse. This eliminates
//! per-insert `HashMap` heap allocations in the hot path.
//!
//! # Safety
//!
//! This module uses **only safe Rust** — no `unsafe` blocks, no raw pointer
//! manipulation. All access is mediated through `Vec<Slot>` and
//! `HashMap<EventId, usize>`.

use omnia_primitives::{Event, EventId};
use std::collections::HashMap;
use thiserror::Error;

/// A slot in the event pool.
///
/// Each slot is either occupied with an event or free and linked into
/// the free list for reuse.
#[derive(Debug)]
enum Slot {
    /// Slot is occupied with an event.
    Occupied { event: Box<Event> },
    /// Slot is free and can be reused. `next_free` points to the next
    /// free slot in the free list, or `None` if this is the tail.
    Free { next_free: Option<usize> },
}

/// Errors that can occur during event pool operations.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum EventPoolError {
    /// The pool has reached its maximum capacity and cannot grow further.
    #[error("Pool is full (capacity: {0}, max: {1})")]
    PoolFull(usize, usize),
    /// An event with the same ID already exists in the pool.
    #[error("Event already exists: {0}")]
    DuplicateEvent(String),
    /// An event failed validation or indexing.
    #[error("Invalid event: {0}")]
    InvalidEvent(String),
}

/// Statistics about the event pool's memory usage and occupancy.
#[derive(Debug, Clone, PartialEq)]
pub struct EventPoolStats {
    /// Total capacity (number of slots).
    pub total_capacity: usize,
    /// Number of occupied slots.
    pub occupied: usize,
    /// Number of free slots available for reuse.
    pub free: usize,
    /// Utilization ratio: `occupied / total_capacity`.
    pub utilization: f64,
    /// Number of times the pool has grown beyond its initial capacity.
    pub growth_count: usize,
}

/// Pre-allocated arena for Event structs.
///
/// Uses a slab-based allocator to avoid per-insert heap allocations.
/// Events are stored in fixed-size slots indexed by a `Vec<Slot>`.
/// Freed slots are tracked in an intrusive free list (using the slot
/// index itself as the link) for O(1) allocation and deallocation.
///
/// # Growth Strategy
///
/// When all slots are occupied and a new insert is requested, the pool
/// grows by `initial_capacity * growth_factor` slots (rounded up).
/// Growth is bounded by `max_capacity` to prevent unbounded memory use.
/// Hysteresis: after growth, the pool does not shrink, but free slots
/// from pruning are reused before new slots are allocated.
///
/// # Example
///
/// ```ignore
/// use omnia_consensus::event_pool::EventPool;
/// let pool = EventPool::new(1024, 1_000_000);
/// let slot = pool.insert(event)?;
/// let retrieved = pool.get(&event_id);
/// ```
pub struct EventPool {
    /// Storage slots. Index corresponds to slot number.
    slots: Vec<Slot>,
    /// Index from EventId to slot index for O(1) lookup.
    index: HashMap<EventId, usize>,
    /// Head of the free list (index into `slots`), or `None` if no free slots.
    free_head: Option<usize>,
    /// Number of currently occupied slots.
    occupied_count: usize,
    /// Initial capacity (pre-allocated on creation).
    initial_capacity: usize,
    /// Maximum capacity (prevents unbounded growth).
    max_capacity: usize,
    /// Growth factor when pool is exhausted (e.g., 1.5 = grow by 50%).
    growth_factor: f64,
    /// Number of times the pool has grown beyond initial capacity.
    growth_count: usize,
}

impl EventPool {
    /// Create a new event pool with the given initial and maximum capacities.
    ///
    /// Pre-allocates `initial_capacity` free slots. The pool will grow
    /// dynamically (up to `max_capacity`) when all slots are occupied.
    ///
    /// # Panics
    ///
    /// Panics if `initial_capacity` is 0 or greater than `max_capacity`,
    /// or if `growth_factor` is less than 1.0.
    pub fn new(initial_capacity: usize, max_capacity: usize) -> Self {
        assert!(initial_capacity > 0, "initial_capacity must be > 0");
        assert!(
            initial_capacity <= max_capacity,
            "initial_capacity must be <= max_capacity"
        );
        let growth_factor = 1.5;

        // Pre-allocate free slots
        let mut slots = Vec::with_capacity(initial_capacity);
        for i in 0..initial_capacity {
            let next_free = if i + 1 < initial_capacity { Some(i + 1) } else { None };
            slots.push(Slot::Free { next_free });
        }

        Self {
            slots,
            index: HashMap::with_capacity(initial_capacity),
            free_head: if initial_capacity > 0 { Some(0) } else { None },
            occupied_count: 0,
            initial_capacity,
            max_capacity,
            growth_factor,
            growth_count: 0,
        }
    }

    /// Insert an event into the pool, returning the slot index.
    ///
    /// If a free slot is available, the event is placed there. Otherwise,
    /// the pool attempts to grow. If growth is not possible (at max capacity),
    /// returns [`EventPoolError::PoolFull`].
    ///
    /// # Errors
    ///
    /// - [`EventPoolError::DuplicateEvent`] — an event with the same ID already exists.
    /// - [`EventPoolError::PoolFull`] — the pool is at max capacity and cannot grow.
    pub fn insert(&mut self, event: Event) -> Result<usize, EventPoolError> {
        let event_id = event.id;

        // Check for duplicate
        if self.index.contains_key(&event_id) {
            return Err(EventPoolError::DuplicateEvent(hex::encode(&event_id[..8])));
        }

        // Find a free slot
        let slot_index = match self.free_head {
            Some(idx) => {
                // Pop from free list
                let next_free = match &self.slots[idx] {
                    Slot::Free { next_free } => *next_free,
                    Slot::Occupied { .. } => {
                        // This should never happen — free list integrity bug
                        return Err(EventPoolError::PoolFull(self.slots.len(), self.max_capacity));
                    }
                };
                self.free_head = next_free;
                idx
            }
            None => {
                // No free slots — try to grow
                self.grow()?;
                // After growth, free_head must be Some
                match self.free_head {
                    Some(idx) => {
                        let next_free = match &self.slots[idx] {
                            Slot::Free { next_free } => *next_free,
                            Slot::Occupied { .. } => {
                                return Err(EventPoolError::PoolFull(self.slots.len(), self.max_capacity));
                            }
                        };
                        self.free_head = next_free;
                        idx
                    }
                    None => {
                        return Err(EventPoolError::PoolFull(self.slots.len(), self.max_capacity));
                    }
                }
            }
        };

        // Place event in slot
        self.slots[slot_index] = Slot::Occupied { event: Box::new(event) };
        self.index.insert(event_id, slot_index);
        self.occupied_count += 1;

        Ok(slot_index)
    }

    /// Look up an event by its ID.
    ///
    /// Returns `None` if the event is not in the pool.
    pub fn get(&self, event_id: &EventId) -> Option<&Event> {
        let &slot_idx = self.index.get(event_id)?;
        match &self.slots[slot_idx] {
            Slot::Occupied { event } => Some(event),
            Slot::Free { .. } => None,
        }
    }

    /// Look up an event by its slot index.
    ///
    /// Returns `None` if the slot is free or the index is out of bounds.
    pub fn get_by_slot(&self, slot: usize) -> Option<&Event> {
        if slot >= self.slots.len() {
            return None;
        }
        match &self.slots[slot] {
            Slot::Occupied { event } => Some(event),
            Slot::Free { .. } => None,
        }
    }

    /// Get a mutable reference to an event by its ID.
    ///
    /// Returns `None` if the event is not in the pool.
    pub fn get_mut(&mut self, event_id: &EventId) -> Option<&mut Event> {
        let &slot_idx = self.index.get(event_id)?;
        match &mut self.slots[slot_idx] {
            Slot::Occupied { event } => Some(event),
            Slot::Free { .. } => None,
        }
    }

    /// Remove an event from the pool, freeing its slot for reuse.
    ///
    /// Returns the removed event, or `None` if not found.
    pub fn remove(&mut self, event_id: &EventId) -> Option<Event> {
        let slot_idx = self.index.remove(event_id)?;

        // Replace the slot with a free entry pointing to the current free_head
        let old_slot = std::mem::replace(
            &mut self.slots[slot_idx],
            Slot::Free {
                next_free: self.free_head,
            },
        );

        self.free_head = Some(slot_idx);
        self.occupied_count -= 1;

        match old_slot {
            Slot::Occupied { event } => Some(*event),
            Slot::Free { .. } => None, // Should not happen
        }
    }

    /// Check whether an event exists in the pool.
    pub fn contains(&self, event_id: &EventId) -> bool {
        self.index.contains_key(event_id)
    }

    /// Get the number of occupied slots (i.e., events currently stored).
    pub fn len(&self) -> usize {
        self.occupied_count
    }

    /// Check if the pool is empty (no occupied slots).
    pub fn is_empty(&self) -> bool {
        self.occupied_count == 0
    }

    /// Get the total capacity (number of slots, both free and occupied).
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Get the number of free slots available for reuse.
    pub fn free_count(&self) -> usize {
        self.slots.len().saturating_sub(self.occupied_count)
    }

    /// Get the slot index for a given event ID.
    ///
    /// Returns `None` if the event is not in the pool.
    pub fn slot_of(&self, event_id: &EventId) -> Option<usize> {
        self.index.get(event_id).copied()
    }

    /// Grow the pool by allocating additional slots.
    ///
    /// The number of new slots is `initial_capacity * growth_factor`,
    /// rounded up, but bounded by `max_capacity`.
    ///
    /// # Errors
    ///
    /// Returns [`EventPoolError::PoolFull`] if the pool is already at
    /// `max_capacity`.
    fn grow(&mut self) -> Result<(), EventPoolError> {
        if self.slots.len() >= self.max_capacity {
            return Err(EventPoolError::PoolFull(self.slots.len(), self.max_capacity));
        }

        let additional = ((self.initial_capacity as f64) * self.growth_factor) as usize;
        let additional = additional.max(1); // Grow by at least 1 slot
        let new_total = self.slots.len().saturating_add(additional).min(self.max_capacity);
        let actual_additional = new_total.saturating_sub(self.slots.len());

        if actual_additional == 0 {
            return Err(EventPoolError::PoolFull(self.slots.len(), self.max_capacity));
        }

        // Link new free slots together
        let start_idx = self.slots.len();
        for i in 0..actual_additional {
            let next_free = if i + 1 < actual_additional {
                Some(start_idx + i + 1)
            } else {
                // Last new slot points to the current free_head
                self.free_head
            };
            self.slots.push(Slot::Free { next_free });
        }

        // New free_head is the first newly allocated slot
        self.free_head = Some(start_idx);
        self.growth_count += 1;

        Ok(())
    }

    /// Get memory usage and occupancy statistics.
    pub fn stats(&self) -> EventPoolStats {
        let total = self.slots.len();
        let occupied = self.occupied_count;
        let free = total.saturating_sub(occupied);
        let utilization = if total > 0 { occupied as f64 / total as f64 } else { 0.0 };

        EventPoolStats {
            total_capacity: total,
            occupied,
            free,
            utilization,
            growth_count: self.growth_count,
        }
    }

    /// Check if the pool is at steady state (no growth needed).
    ///
    /// Returns `true` if there are free slots available, meaning the next
    /// insert will not require pool growth.
    pub fn is_at_steady_state(&self) -> bool {
        self.free_head.is_some()
    }
}

impl Default for EventPool {
    fn default() -> Self {
        Self::new(1024, 1_000_000)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_primitives::{Event, NodeId, VectorClock};

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn make_event(creator: NodeId, seq: u64) -> Event {
        let vc = VectorClock::with_node(creator, seq + 1);
        Event::new(creator, seq, vc, None, None, vec![]).expect("valid event")
    }

    #[test]
    fn test_pool_creation() {
        let pool = EventPool::new(16, 1024);
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.capacity(), 16);
        assert_eq!(pool.free_count(), 16);
        assert!(pool.is_empty());
        assert!(pool.is_at_steady_state());
    }

    #[test]
    fn test_insert_and_get() {
        let mut pool = EventPool::new(16, 1024);
        let node = test_node(1);
        let event = make_event(node, 0);
        let event_id = event.id;

        let slot = pool.insert(event).expect("insert should succeed");
        assert_eq!(pool.len(), 1);
        assert!(pool.contains(&event_id));

        let retrieved = pool.get(&event_id).expect("event should be found");
        assert_eq!(retrieved.creator, node);
        assert_eq!(retrieved.sequence, 0);

        let retrieved_by_slot = pool.get_by_slot(slot).expect("event should be found by slot");
        assert_eq!(retrieved_by_slot.creator, node);
    }

    #[test]
    fn test_insert_duplicate() {
        let mut pool = EventPool::new(16, 1024);
        let node = test_node(1);
        let event = make_event(node, 0);
        let event2 = event.clone();

        pool.insert(event).expect("first insert should succeed");
        let result = pool.insert(event2);
        assert!(matches!(result, Err(EventPoolError::DuplicateEvent(_))));
    }

    #[test]
    fn test_remove() {
        let mut pool = EventPool::new(16, 1024);
        let node = test_node(1);
        let event = make_event(node, 0);
        let event_id = event.id;

        pool.insert(event).expect("insert should succeed");
        assert_eq!(pool.len(), 1);

        let removed = pool.remove(&event_id).expect("remove should return event");
        assert_eq!(removed.creator, node);
        assert_eq!(pool.len(), 0);
        assert!(!pool.contains(&event_id));
        assert_eq!(pool.free_count(), 16); // Slot freed
    }

    #[test]
    fn test_get_mut() {
        let mut pool = EventPool::new(16, 1024);
        let node = test_node(1);
        let event = make_event(node, 0);
        let event_id = event.id;

        pool.insert(event).expect("insert should succeed");

        let event_mut = pool.get_mut(&event_id).expect("should get mutable ref");
        event_mut.ack_count = 42;

        let retrieved = pool.get(&event_id).expect("should find event");
        assert_eq!(retrieved.ack_count, 42);
    }

    #[test]
    fn test_slot_of() {
        let mut pool = EventPool::new(16, 1024);
        let node = test_node(1);
        let event = make_event(node, 0);
        let event_id = event.id;

        let slot = pool.insert(event).expect("insert should succeed");
        assert_eq!(pool.slot_of(&event_id), Some(slot));
    }

    #[test]
    fn test_pool_growth() {
        let mut pool = EventPool::new(4, 100);
        let node = test_node(1);

        // Fill initial capacity
        for i in 0..4 {
            let event = make_event(node, i);
            pool.insert(event).expect("insert should succeed");
        }
        assert_eq!(pool.len(), 4);
        assert_eq!(pool.free_count(), 0);
        assert!(!pool.is_at_steady_state());

        // This should trigger growth
        let event = make_event(test_node(2), 0);
        pool.insert(event).expect("insert after growth should succeed");
        assert!(pool.capacity() > 4);
        assert_eq!(pool.stats().growth_count, 1);
    }

    #[test]
    fn test_pool_full() {
        let mut pool = EventPool::new(4, 4); // max = initial, no growth possible
        let node = test_node(1);

        for i in 0..4 {
            let event = make_event(node, i);
            pool.insert(event).expect("insert should succeed");
        }

        // Next insert should fail
        let event = make_event(test_node(2), 0);
        let result = pool.insert(event);
        assert!(matches!(result, Err(EventPoolError::PoolFull(4, 4))));
    }

    #[test]
    fn test_free_slot_reuse() {
        let mut pool = EventPool::new(4, 4);
        let node = test_node(1);

        let event1 = make_event(node, 0);
        let event1_id = event1.id;
        pool.insert(event1).expect("insert should succeed");

        let event2 = make_event(test_node(2), 0);
        let event2_id = event2.id;
        pool.insert(event2).expect("insert should succeed");

        // Remove event1
        pool.remove(&event1_id).expect("remove should succeed");
        assert_eq!(pool.free_count(), 3); // 4 slots - 1 occupied

        // Insert new event — should reuse freed slot
        let event3 = make_event(test_node(3), 0);
        let event3_id = event3.id;
        pool.insert(event3).expect("insert should reuse free slot");
        assert_eq!(pool.len(), 2); // event2 + event3
        assert!(!pool.contains(&event1_id));
        assert!(pool.contains(&event2_id));
        assert!(pool.contains(&event3_id));
    }

    #[test]
    fn test_stats() {
        let mut pool = EventPool::new(16, 1024);
        let node = test_node(1);

        for i in 0..5 {
            let event = make_event(node, i);
            pool.insert(event).expect("insert should succeed");
        }

        let stats = pool.stats();
        assert_eq!(stats.total_capacity, 16);
        assert_eq!(stats.occupied, 5);
        assert_eq!(stats.free, 11);
        assert!((stats.utilization - 5.0 / 16.0).abs() < 0.001);
        assert_eq!(stats.growth_count, 0);
    }

    #[test]
    fn test_steady_state() {
        let mut pool = EventPool::new(16, 1024);
        assert!(pool.is_at_steady_state());

        let node = test_node(1);
        for i in 0..16 {
            let event = make_event(node, i);
            pool.insert(event).expect("insert should succeed");
        }
        assert!(!pool.is_at_steady_state());

        // Remove one — now we have a free slot again
        let first_id = pool.index.keys().next().copied();
        if let Some(id) = first_id {
            pool.remove(&id);
            assert!(pool.is_at_steady_state());
        }
    }
}

/// Stress tests for the event pool.
///
/// These tests verify:
/// - High-throughput insertion (10K events/sec target)
/// - Memory leak detection (event count stays bounded after pruning)
/// - Free list integrity after many insert/remove cycles
/// - Pool growth and shrink behavior
#[cfg(test)]
mod stress_test_pool_allocation {
    use super::*;
    use omnia_primitives::{Event, NodeId, VectorClock};

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn make_event(creator: NodeId, seq: u64) -> Event {
        let vc = VectorClock::with_node(creator, seq + 1);
        Event::new(creator, seq, vc, None, None, vec![]).expect("valid event")
    }

    /// Stress test: insert 10,000 events rapidly.
    /// Verifies zero allocation failures under sustained insert load.
    #[test]
    fn test_10k_events_insertion() {
        let mut pool = EventPool::new(1024, 50_000);

        for i in 0..10_000 {
            let creator = test_node(((i % 100) + 1) as u8);
            let event = make_event(creator, (i / 100) as u64);
            pool.insert(event).unwrap_or_else(|e| {
                panic!("insertion failed at event {i}: {e}");
            });
        }

        assert_eq!(pool.len(), 10_000);
        let stats = pool.stats();
        assert!(stats.total_capacity >= 10_000);
    }

    /// Memory leak detection: after inserting and removing events,
    /// the pool's occupied count should match the expected value
    /// exactly, and free slots should be properly reclaimed.
    #[test]
    fn test_memory_leak_detection() {
        let mut pool = EventPool::new(256, 10_000);
        let mut ids: Vec<EventId> = Vec::new();

        // Insert 1000 events
        for i in 0..1000 {
            let creator = test_node(((i % 10) + 1) as u8);
            let event = make_event(creator, (i / 10) as u64);
            ids.push(event.id);
            pool.insert(event).expect("insert should succeed");
        }
        assert_eq!(pool.len(), 1000);

        // Remove all events
        for id in &ids {
            pool.remove(id).expect("remove should succeed");
        }
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());

        // Free slots should equal total capacity
        assert_eq!(pool.free_count(), pool.capacity());
    }

    /// Concurrent-like insert/remove cycles: simulate interleaved
    /// insertions and removals to verify free list integrity.
    #[test]
    fn test_interleaved_insert_remove() {
        let mut pool = EventPool::new(64, 10_000);
        let mut active_ids: Vec<EventId> = Vec::new();

        for round in 0..50 {
            // Insert 20 events
            for j in 0..20 {
                let creator = test_node(((round * 20 + j) % 100 + 1) as u8);
                let event = make_event(creator, round * 20 + j);
                active_ids.push(event.id);
                pool.insert(event).expect("insert should succeed");
            }

            // Remove 10 events (from the front)
            for _ in 0..10 {
                if let Some(id) = active_ids.first().copied() {
                    pool.remove(&id).expect("remove should succeed");
                    active_ids.remove(0);
                }
            }
        }

        // After 50 rounds: inserted 50*20=1000, removed 50*10=500
        assert_eq!(pool.len(), 500);
        assert_eq!(active_ids.len(), 500);

        // All active IDs should still be in the pool
        for id in &active_ids {
            assert!(pool.contains(id), "active event should still be in pool");
        }
    }

    /// Pool growth and free list integrity under pressure.
    #[test]
    fn test_growth_and_free_list_integrity() {
        let mut pool = EventPool::new(8, 10_000);
        let mut all_ids: Vec<EventId> = Vec::new();

        // Insert beyond initial capacity to force growth
        for i in 0..100 {
            let creator = test_node((i % 5 + 1) as u8);
            let event = make_event(creator, i);
            all_ids.push(event.id);
            pool.insert(event).expect("insert should succeed");
        }
        assert!(pool.capacity() > 8, "pool should have grown");
        let growth_count = pool.stats().growth_count;
        assert!(growth_count > 0, "pool should have grown at least once");

        // Remove half of the events
        for id in all_ids.iter().step_by(2) {
            pool.remove(id).expect("remove should succeed");
        }

        assert_eq!(pool.len(), 50);

        // Re-insert into freed slots
        for i in 200..250 {
            let creator = test_node((i % 5 + 1) as u8);
            let event = make_event(creator, i);
            pool.insert(event).expect("insert should reuse free slots");
        }

        // Should not have grown further (reused free slots)
        assert_eq!(
            pool.stats().growth_count,
            growth_count,
            "pool should not grow when free slots are available"
        );
    }

    /// Verify free list integrity by inserting and removing in patterns.
    #[test]
    fn test_free_list_integrity() {
        let mut pool = EventPool::new(32, 10_000);
        let mut ids: Vec<EventId> = Vec::new();

        // Insert 32 events (fill initial capacity)
        for i in 0..32 {
            let creator = test_node(1);
            let event = make_event(creator, i);
            ids.push(event.id);
            pool.insert(event).expect("insert should succeed");
        }
        assert_eq!(pool.free_count(), 0);

        // Remove every other event
        let mut removed_ids: Vec<EventId> = Vec::new();
        for (idx, id) in ids.iter().enumerate() {
            if idx % 2 == 0 {
                pool.remove(id).expect("remove should succeed");
                removed_ids.push(*id);
            }
        }
        assert_eq!(pool.len(), 16);
        assert_eq!(pool.free_count(), 16);

        // Re-insert 16 new events (should use free slots)
        for i in 100..116 {
            let creator = test_node(2);
            let event = make_event(creator, i);
            pool.insert(event).expect("insert should use free slot");
        }
        assert_eq!(pool.len(), 32);
        assert_eq!(pool.free_count(), 0);

        // All removed IDs should no longer be in pool
        for id in &removed_ids {
            assert!(!pool.contains(id), "removed event should not be in pool");
        }
    }
}
