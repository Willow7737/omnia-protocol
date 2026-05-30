//! Pre-indexed vector clocks for O(1) parent resolution
//!
//! Replaces HashMap parent lookups with pre-computed index structures
//! that allow O(1) resolution of parent events using vector clock
//! comparisons instead of hash lookups.
//!
//! # Design
//!
//! The key insight: instead of looking up parents via `HashMap<EventId, Event>`
//! (which requires hashing + equality check on a 32-byte key), we pre-index
//! events by `(creator, sequence)` pairs. Since each event has a monotonic
//! sequence number per creator, we can use a two-level index:
//!
//! 1. `creator_index: HashMap<NodeId, Vec<Option<usize>>>` — for each creator,
//!    the slot index at each sequence number.
//! 2. `slot_to_id: Vec<Option<EventId>>` — reverse mapping from slot index
//!    to event ID.
//!
//! This makes `resolve_parent(creator, sequence)` an O(1) operation:
//! look up the creator's vector, then index by sequence number.

use omnia_primitives::{EventId, NodeId};
use std::collections::HashMap;

/// Maximum allowed sequence number for indexing.
///
/// This bound prevents unbounded memory allocation when a `u64` sequence
/// number is cast to `usize` for vector indexing. A sequence number above
/// this limit would require allocating a Vec of size > 1M entries per creator.
pub const MAX_SEQUENCE: u64 = 1_000_000;

/// Error type for vector clock index operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum VectorClockIndexError {
    /// The sequence number exceeds the maximum allowed value.
    #[error("sequence number {sequence} exceeds maximum {max} for creator {creator:?}")]
    SequenceTooLarge {
        /// The creator whose sequence was too large.
        creator: NodeId,
        /// The sequence number that was too large.
        sequence: u64,
        /// The maximum allowed sequence number.
        max: u64,
    },
}

/// Statistics about the vector clock index.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorClockIndexStats {
    /// Number of distinct creators tracked.
    pub creator_count: usize,
    /// Total number of indexed (creator, sequence) entries.
    pub total_entries: usize,
    /// Total number of slot-to-ID reverse mappings.
    pub slot_mappings: usize,
    /// Approximate memory usage in bytes.
    pub approx_memory_bytes: usize,
}

/// Pre-indexed vector clock for O(1) parent resolution.
///
/// Maintains a two-level index:
/// - **Forward**: `(creator, sequence) → slot index` — used for parent
///   resolution during DAG insertion.
/// - **Reverse**: `slot index → EventId` — used for cleanup when events
///   are removed from the pool.
///
/// # Parent Resolution
///
/// In the original `CausalGraph`, resolving a parent event requires a
/// `HashMap::get(&parent_id)` which involves:
/// 1. SHA-256 hash of the 32-byte key
/// 2. Equality comparison
/// 3. Collision resolution
///
/// With the `VectorClockIndex`, if we know the creator and sequence of
/// the parent, resolution is:
/// 1. `HashMap::get(&creator)` — first-level lookup
/// 2. `Vec[sequence]` — direct index
///
/// The second step is a simple array access with no hashing.
pub struct VectorClockIndex {
    /// Index: `creator → sequence → slot index` in the EventPool.
    /// Each creator has a sparse vector where `vec[seq] = Some(slot_index)`.
    creator_index: HashMap<NodeId, Vec<Option<usize>>>,
    /// Reverse index: `slot index → EventId`.
    /// Used for cleanup when events are removed.
    slot_to_id: Vec<Option<EventId>>,
}

impl VectorClockIndex {
    /// Create a new, empty vector clock index.
    pub fn new() -> Self {
        Self {
            creator_index: HashMap::new(),
            slot_to_id: Vec::new(),
        }
    }

    /// Create a new vector clock index with pre-allocated capacity.
    ///
    /// Pre-allocates space for `expected_creators` distinct creators
    /// and `expected_events` slot mappings.
    pub fn with_capacity(expected_creators: usize, expected_events: usize) -> Self {
        Self {
            creator_index: HashMap::with_capacity(expected_creators),
            slot_to_id: Vec::with_capacity(expected_events),
        }
    }

    /// Index an event by its creator and sequence number.
    ///
    /// After calling this method, `resolve_parent(creator, sequence)` will
    /// return `(event_id, slot)`.
    ///
    /// # Arguments
    ///
    /// * `creator` — The node ID of the event's creator.
    /// * `sequence` — The monotonic sequence number of the event.
    /// * `slot` — The slot index in the `EventPool`.
    /// * `event_id` — The event's unique identifier (hash).
    ///
    /// # Errors
    ///
    /// Returns [`VectorClockIndexError::SequenceTooLarge`] if `sequence`
    /// exceeds [`MAX_SEQUENCE`].
    pub fn index_event(&mut self, creator: &NodeId, sequence: u64, slot: usize, event_id: EventId) -> Result<(), VectorClockIndexError> {
        // Bounds check: prevent unbounded memory allocation from u64 → usize cast
        if sequence > MAX_SEQUENCE {
            return Err(VectorClockIndexError::SequenceTooLarge {
                creator: *creator,
                sequence,
                max: MAX_SEQUENCE,
            });
        }

        let seq_idx = sequence as usize;

        // Update creator index
        let seq_vec = self.creator_index.entry(*creator).or_default();
        if seq_idx >= seq_vec.len() {
            seq_vec.resize(seq_idx + 1, None);
        }
        seq_vec[seq_idx] = Some(slot);

        // Update reverse index
        if slot >= self.slot_to_id.len() {
            self.slot_to_id.resize(slot + 1, None);
        }
        self.slot_to_id[slot] = Some(event_id);

        Ok(())
    }

    /// Resolve a parent event by creator and sequence number.
    ///
    /// Returns `Some((event_id, slot_index))` if the (creator, sequence)
    /// pair is indexed, or `None` if not found.
    ///
    /// This is an O(1) operation: hash map lookup on creator, then
    /// direct vector indexing on sequence.
    pub fn resolve_parent(&self, creator: &NodeId, sequence: u64) -> Option<(EventId, usize)> {
        let seq_vec = self.creator_index.get(creator)?;
        let seq_idx = sequence as usize;
        let slot = seq_vec.get(seq_idx)?.as_ref()?;
        let event_id = self.slot_to_id.get(*slot)?.as_ref()?.to_owned();
        Some((event_id, *slot))
    }

    /// Resolve only the slot index for a (creator, sequence) pair.
    ///
    /// Returns `Some(slot_index)` if indexed, `None` otherwise.
    pub fn resolve_slot(&self, creator: &NodeId, sequence: u64) -> Option<usize> {
        let seq_vec = self.creator_index.get(creator)?;
        let seq_idx = sequence as usize;
        seq_vec.get(seq_idx)?.as_ref().copied()
    }

    /// Remove an event from the index by its slot.
    ///
    /// Clears both the forward and reverse mappings for the given slot.
    /// Returns the removed `EventId` if the slot was occupied.
    pub fn remove_event(&mut self, slot: usize) -> Option<EventId> {
        if slot >= self.slot_to_id.len() {
            return None;
        }

        let event_id = self.slot_to_id[slot].take()?;

        // We don't remove from creator_index here because finding the
        // creator and sequence for a given slot would require additional
        // tracking. Instead, the forward index entries become stale but
        // harmless — they point to a slot that now has None in slot_to_id.
        // resolve_parent will return None for such entries because slot_to_id
        // is checked.

        Some(event_id)
    }

    /// Remove an event from the index by creator and sequence.
    ///
    /// This is more thorough than `remove_event(slot)` because it also
    /// clears the forward index entry. Use this when both the creator
    /// and sequence are known.
    pub fn remove_by_creator_sequence(&mut self, creator: &NodeId, sequence: u64) -> Option<usize> {
        let seq_vec = self.creator_index.get_mut(creator)?;
        let seq_idx = sequence as usize;

        let slot = seq_vec.get_mut(seq_idx)?.take()?;

        // Clear reverse mapping
        if slot < self.slot_to_id.len() {
            self.slot_to_id[slot] = None;
        }

        Some(slot)
    }

    /// Check whether a (creator, sequence) pair is indexed.
    pub fn contains(&self, creator: &NodeId, sequence: u64) -> bool {
        self.resolve_parent(creator, sequence).is_some()
    }

    /// Get the number of distinct creators tracked.
    pub fn creator_count(&self) -> usize {
        self.creator_index.len()
    }

    /// Get the highest sequence number for a given creator.
    ///
    /// Returns `None` if the creator is not indexed.
    pub fn max_sequence(&self, creator: &NodeId) -> Option<u64> {
        let seq_vec = self.creator_index.get(creator)?;
        // Find the last Some(entry) in the vector
        for i in (0..seq_vec.len()).rev() {
            if seq_vec[i].is_some() {
                return Some(i as u64);
            }
        }
        None
    }

    /// Get statistics about the index.
    pub fn stats(&self) -> VectorClockIndexStats {
        let total_entries: usize = self
            .creator_index
            .values()
            .map(|v| v.iter().filter(|e| e.is_some()).count())
            .sum();

        let slot_mappings: usize = self.slot_to_id.iter().filter(|e| e.is_some()).count();

        // Approximate memory usage:
        // - creator_index: HashMap overhead + NodeId keys + Vec values
        // - slot_to_id: Vec<Option<EventId>>
        let creator_index_bytes = self.creator_index.capacity() * (32 + std::mem::size_of::<Vec<Option<usize>>>())
            + self
                .creator_index
                .values()
                .map(|v| v.capacity() * std::mem::size_of::<Option<usize>>())
                .sum::<usize>();
        let slot_to_id_bytes = self.slot_to_id.capacity() * std::mem::size_of::<Option<EventId>>();

        VectorClockIndexStats {
            creator_count: self.creator_index.len(),
            total_entries,
            slot_mappings,
            approx_memory_bytes: creator_index_bytes + slot_to_id_bytes,
        }
    }
}

impl Default for VectorClockIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn test_event_id(id: u8) -> EventId {
        let mut eid = [0u8; 32];
        eid[0] = id;
        eid
    }

    #[test]
    fn test_index_and_resolve() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);
        let event_id = test_event_id(42);

        index.index_event(&creator, 0, 5, event_id).unwrap();

        let result = index.resolve_parent(&creator, 0);
        assert_eq!(result, Some((event_id, 5)));
    }

    #[test]
    fn test_resolve_missing() {
        let index = VectorClockIndex::new();
        let creator = test_node(1);

        assert_eq!(index.resolve_parent(&creator, 0), None);
    }

    #[test]
    fn test_multiple_sequences() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);

        for seq in 0..5u64 {
            let mut eid = [0u8; 32];
            eid[0] = seq as u8;
            index.index_event(&creator, seq, seq as usize * 10, eid).unwrap();
        }

        for seq in 0..5u64 {
            let mut eid = [0u8; 32];
            eid[0] = seq as u8;
            let result = index.resolve_parent(&creator, seq);
            assert_eq!(result, Some((eid, seq as usize * 10)));
        }

        // Unindexed sequence
        assert_eq!(index.resolve_parent(&creator, 5), None);
    }

    #[test]
    fn test_multiple_creators() {
        let mut index = VectorClockIndex::new();
        let creator_a = test_node(1);
        let creator_b = test_node(2);

        index.index_event(&creator_a, 0, 0, test_event_id(1)).unwrap();
        index.index_event(&creator_b, 0, 1, test_event_id(2)).unwrap();
        index.index_event(&creator_a, 1, 2, test_event_id(3)).unwrap();

        assert_eq!(index.resolve_parent(&creator_a, 0), Some((test_event_id(1), 0)));
        assert_eq!(index.resolve_parent(&creator_b, 0), Some((test_event_id(2), 1)));
        assert_eq!(index.resolve_parent(&creator_a, 1), Some((test_event_id(3), 2)));
    }

    #[test]
    fn test_remove_by_slot() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);

        index.index_event(&creator, 0, 5, test_event_id(42)).unwrap();
        assert_eq!(index.resolve_parent(&creator, 0), Some((test_event_id(42), 5)));

        let removed = index.remove_event(5);
        assert_eq!(removed, Some(test_event_id(42)));

        // After removal, resolve should return None
        assert_eq!(index.resolve_parent(&creator, 0), None);
    }

    #[test]
    fn test_remove_by_creator_sequence() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);

        index.index_event(&creator, 0, 5, test_event_id(42)).unwrap();
        index.index_event(&creator, 1, 6, test_event_id(43)).unwrap();

        let slot = index.remove_by_creator_sequence(&creator, 0);
        assert_eq!(slot, Some(5));

        // seq 0 should be gone
        assert_eq!(index.resolve_parent(&creator, 0), None);
        // seq 1 should still be there
        assert_eq!(index.resolve_parent(&creator, 1), Some((test_event_id(43), 6)));
    }

    #[test]
    fn test_contains() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);

        assert!(!index.contains(&creator, 0));

        index.index_event(&creator, 0, 5, test_event_id(42)).unwrap();
        assert!(index.contains(&creator, 0));
    }

    #[test]
    fn test_max_sequence() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);

        assert_eq!(index.max_sequence(&creator), None);

        index.index_event(&creator, 0, 0, test_event_id(1)).unwrap();
        assert_eq!(index.max_sequence(&creator), Some(0));

        index.index_event(&creator, 3, 1, test_event_id(2)).unwrap();
        assert_eq!(index.max_sequence(&creator), Some(3));

        index.index_event(&creator, 7, 2, test_event_id(3)).unwrap();
        assert_eq!(index.max_sequence(&creator), Some(7));
    }

    #[test]
    fn test_stats() {
        let mut index = VectorClockIndex::with_capacity(10, 100);
        let creator_a = test_node(1);
        let creator_b = test_node(2);

        index.index_event(&creator_a, 0, 0, test_event_id(1)).unwrap();
        index.index_event(&creator_a, 1, 1, test_event_id(2)).unwrap();
        index.index_event(&creator_b, 0, 2, test_event_id(3)).unwrap();

        let stats = index.stats();
        assert_eq!(stats.creator_count, 2);
        assert_eq!(stats.total_entries, 3);
        assert_eq!(stats.slot_mappings, 3);
        assert!(stats.approx_memory_bytes > 0);
    }

    #[test]
    fn test_resolve_slot() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);

        index.index_event(&creator, 5, 42, test_event_id(1)).unwrap();

        assert_eq!(index.resolve_slot(&creator, 5), Some(42));
        assert_eq!(index.resolve_slot(&creator, 0), None);
    }

    #[test]
    fn test_high_sequence_numbers() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);

        // Test with a high sequence number to verify sparse vector handling
        index.index_event(&creator, 1000, 0, test_event_id(1)).unwrap();
        assert_eq!(index.resolve_parent(&creator, 1000), Some((test_event_id(1), 0)));

        // Lower sequences should be None (not indexed)
        assert_eq!(index.resolve_parent(&creator, 0), None);
        assert_eq!(index.resolve_parent(&creator, 999), None);
    }

    #[test]
    fn test_sequence_too_large() {
        let mut index = VectorClockIndex::new();
        let creator = test_node(1);

        // Sequence beyond MAX_SEQUENCE should be rejected
        let result = index.index_event(&creator, MAX_SEQUENCE + 1, 0, test_event_id(1));
        assert!(result.is_err());
        match result.unwrap_err() {
            VectorClockIndexError::SequenceTooLarge { creator: c, sequence, max } => {
                assert_eq!(c, creator);
                assert_eq!(sequence, MAX_SEQUENCE + 1);
                assert_eq!(max, MAX_SEQUENCE);
            }
        }

        // Sequence at exactly MAX_SEQUENCE should succeed
        let result = index.index_event(&creator, MAX_SEQUENCE, 0, test_event_id(2));
        assert!(result.is_ok());
    }
}
