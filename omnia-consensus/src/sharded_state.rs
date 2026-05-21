//! Sharded Consensus State for Parallel Event Processing
//!
//! Shards consensus state by event hash (first byte of EventId) to enable
//! concurrent event processing. Each shard is protected by its own RwLock,
//! allowing read-heavy workloads to proceed in parallel.
//!
//! # Sharding Strategy
//!
//! The first byte of the `EventId` (a SHA-256 hash) determines which shard
//! an event belongs to. Since SHA-256 output is uniformly distributed, events
//! will be evenly distributed across 256 shards with high probability.
//!
//! # Locking Strategy
//!
//! - **Per-shard RwLock**: `event_states`, `event_rounds`, and `fame_status`
//!   are sharded, each protected by its own `RwLock`. This allows concurrent
//!   reads and writes to different shards.
//!
//! - **Global RwLock**: `round_witnesses`, `node_info`,
//!   `first_event_for_sequence`, and `committed_count` require cross-shard
//!   coordination and are protected by a single global `RwLock`.
//!
//! # Poison Recovery
//!
//! All lock acquisitions recover from poisoning. If a thread panics while
//! holding a lock, the data may be in an inconsistent state, but we recover
//! the lock anyway to prevent a single panic from deadlocking the entire
//! system. This is acceptable because:
//! 1. The consensus state can be rebuilt from the causal graph
//! 2. A panic indicates a bug, which should be fixed regardless
//! 3. Deadlock prevention is more important than perfect consistency after a panic

use crate::consensus::{ConsensusState, NodeConsensusInfo, DEFAULT_COMMITTED_ROUND_THRESHOLD};
use omnia_primitives::{EventId, NodeId};
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

/// Number of shards (256, one per possible first byte of EventId)
const NUM_SHARDS: usize = 256;

/// A single shard of consensus state.
///
/// Each shard holds the per-event data that can be partitioned by EventId:
/// consensus state, round assignment, and fame status. Operations on different
/// shards can proceed concurrently without locking each other.
struct ConsensusShard {
    /// Consensus state for events in this shard
    event_states: HashMap<EventId, ConsensusState>,
    /// Round assignments for events in this shard
    event_rounds: HashMap<EventId, u64>,
    /// Fame status for witnesses in this shard
    fame_status: HashMap<EventId, bool>,
}

impl ConsensusShard {
    fn new() -> Self {
        Self {
            event_states: HashMap::new(),
            event_rounds: HashMap::new(),
            fame_status: HashMap::new(),
        }
    }
}

/// Global state shared across all shards (requires careful synchronization).
///
/// This state cannot be sharded because it is keyed by non-EventId keys
/// (round numbers, node IDs, or composite keys) and needs cross-shard
/// coordination for operations like recording witnesses.
struct GlobalConsensusState {
    /// Witnesses per round (needs cross-shard coordination)
    round_witnesses: HashMap<u64, HashSet<EventId>>,
    /// Per-node consensus info
    node_info: HashMap<NodeId, NodeConsensusInfo>,
    /// Equivocation tracking: (creator, sequence) -> first EventId
    first_event_for_sequence: HashMap<(NodeId, u64), EventId>,
    /// Total committed count
    committed_count: u64,
}

impl GlobalConsensusState {
    fn new() -> Self {
        Self {
            round_witnesses: HashMap::new(),
            node_info: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            committed_count: 0,
        }
    }
}

/// Sharded consensus state with RwLock-protected shards.
///
/// This is a NEW parallel data structure that coexists with the existing
/// [`ConsensusEngine`]. It provides the same state tracking capabilities
/// but with fine-grained locking for concurrent access.
///
/// # Thread Safety
///
/// All methods are `&self` (not `&mut self`) because mutation is done
/// through interior mutability via `RwLock`. Multiple threads can read
/// from different shards concurrently; writes to different shards also
/// proceed concurrently. Only cross-shard operations (witness recording,
/// node info updates) need the global lock.
pub struct ShardedConsensusState {
    /// Sharded state, each protected by its own RwLock
    shards: Vec<RwLock<ConsensusShard>>,
    /// Global state protected by a single RwLock
    global: RwLock<GlobalConsensusState>,
}

/// Statistics for the sharded consensus state.
#[derive(Debug, Clone)]
pub struct ShardedConsensusStats {
    /// Total number of events being tracked across all shards
    pub total_tracked: usize,
    /// Total number of committed events
    pub committed: u64,
    /// Number of events per shard (index = shard index)
    pub shard_loads: Vec<usize>,
}

impl ShardedConsensusState {
    /// Create a new sharded consensus state with 256 shards.
    ///
    /// Each shard is initialized with empty HashMaps. The global state
    /// is also initialized empty.
    pub fn new() -> Self {
        let shards = (0..NUM_SHARDS)
            .map(|_| RwLock::new(ConsensusShard::new()))
            .collect();
        Self {
            shards,
            global: RwLock::new(GlobalConsensusState::new()),
        }
    }

    /// Determine which shard an event belongs to based on its EventId.
    ///
    /// Uses the first byte of the EventId (SHA-256 hash), which provides
    /// a uniform distribution across 256 shards.
    #[inline]
    pub fn shard_index(event_id: &EventId) -> usize {
        event_id[0] as usize
    }

    /// Insert a consensus state for an event into the correct shard.
    ///
    /// If the event already has a state in the shard, it is overwritten.
    pub fn insert_event_state(&self, event_id: EventId, state: ConsensusState) {
        let idx = Self::shard_index(&event_id);
        let mut shard = self.shards[idx].write().unwrap_or_else(|e| e.into_inner());
        shard.event_states.insert(event_id, state);
    }

    /// Read the consensus state for an event from the correct shard.
    ///
    /// Returns `None` if the event has not been tracked.
    pub fn get_event_state(&self, event_id: &EventId) -> Option<ConsensusState> {
        let idx = Self::shard_index(event_id);
        let shard = self.shards[idx].read().unwrap_or_else(|e| e.into_inner());
        shard.event_states.get(event_id).copied()
    }

    /// Insert a round assignment for an event into the correct shard.
    ///
    /// If the event already has a round in the shard, it is overwritten.
    pub fn insert_event_round(&self, event_id: EventId, round: u64) {
        let idx = Self::shard_index(&event_id);
        let mut shard = self.shards[idx].write().unwrap_or_else(|e| e.into_inner());
        shard.event_rounds.insert(event_id, round);
    }

    /// Read the round assignment for an event from the correct shard.
    ///
    /// Returns `None` if the event has not been assigned a round.
    pub fn get_event_round(&self, event_id: &EventId) -> Option<u64> {
        let idx = Self::shard_index(event_id);
        let shard = self.shards[idx].read().unwrap_or_else(|e| e.into_inner());
        shard.event_rounds.get(event_id).copied()
    }

    /// Insert a fame status for a witness into the correct shard.
    pub fn insert_fame_status(&self, event_id: EventId, famous: bool) {
        let idx = Self::shard_index(&event_id);
        let mut shard = self.shards[idx].write().unwrap_or_else(|e| e.into_inner());
        shard.fame_status.insert(event_id, famous);
    }

    /// Read the fame status for a witness from the correct shard.
    pub fn get_fame_status(&self, event_id: &EventId) -> Option<bool> {
        let idx = Self::shard_index(event_id);
        let shard = self.shards[idx].read().unwrap_or_else(|e| e.into_inner());
        shard.fame_status.get(event_id).copied()
    }

    /// Check if an event already exists in the sharded state.
    pub fn contains_event(&self, event_id: &EventId) -> bool {
        let idx = Self::shard_index(event_id);
        let shard = self.shards[idx].read().unwrap_or_else(|e| e.into_inner());
        shard.event_states.contains_key(event_id)
    }

    /// Record a witness for a given round (global lock required).
    ///
    /// This operation requires the global lock because `round_witnesses`
    /// is keyed by round number, not by EventId, and multiple shards may
    /// contribute witnesses to the same round.
    pub fn record_witness(&self, round: u64, event_id: EventId) {
        let mut global = self.global.write().unwrap_or_else(|e| e.into_inner());
        global.round_witnesses.entry(round).or_default().insert(event_id);
    }

    /// Get the set of witnesses for a given round (global read lock).
    ///
    /// Returns an empty set if no witnesses have been recorded for the round.
    pub fn get_witnesses_for_round(&self, round: u64) -> HashSet<EventId> {
        let global = self.global.read().unwrap_or_else(|e| e.into_inner());
        global.round_witnesses.get(&round).cloned().unwrap_or_default()
    }

    /// Check if an event is committed (final).
    ///
    /// Returns `true` only if the event's state is [`ConsensusState::Committed`].
    pub fn is_committed(&self, event_id: &EventId) -> bool {
        matches!(self.get_event_state(event_id), Some(ConsensusState::Committed))
    }

    /// Increment the committed event counter (global lock required).
    pub fn increment_committed(&self, count: u64) {
        let mut global = self.global.write().unwrap_or_else(|e| e.into_inner());
        global.committed_count += count;
    }

    /// Read the total committed event count (global read lock).
    pub fn committed_count(&self) -> u64 {
        let global = self.global.read().unwrap_or_else(|e| e.into_inner());
        global.committed_count
    }

    /// Update node consensus info using a closure (global lock required).
    ///
    /// The closure receives a mutable reference to the [`NodeConsensusInfo`]
    /// for the given node. If the node does not yet have an entry, a default
    /// one is created first.
    pub fn update_node_info(&self, node_id: NodeId, f: impl FnOnce(&mut NodeConsensusInfo)) {
        let mut global = self.global.write().unwrap_or_else(|e| e.into_inner());
        let info = global.node_info.entry(node_id).or_default();
        f(info);
    }

    /// Get the current round for a node (global read lock).
    ///
    /// Returns `0` if the node has not been seen.
    pub fn get_node_round(&self, node_id: &NodeId) -> u64 {
        let global = self.global.read().unwrap_or_else(|e| e.into_inner());
        global.node_info.get(node_id).map(|i| i.current_round).unwrap_or(0)
    }

    /// Record the first event seen for a (creator, sequence) pair for
    /// equivocation tracking (global lock required).
    ///
    /// If an entry already exists for this key, it is NOT overwritten —
    /// the first event wins. This ensures that equivocation detection can
    /// always reference the original event.
    pub fn record_first_sequence(&self, key: (NodeId, u64), event_id: EventId) {
        let mut global = self.global.write().unwrap_or_else(|e| e.into_inner());
        global.first_event_for_sequence.entry(key).or_insert(event_id);
    }

    /// Look up the first event for a (creator, sequence) pair (global read lock).
    ///
    /// Returns `None` if no event has been recorded for this pair.
    pub fn get_first_sequence(&self, key: &(NodeId, u64)) -> Option<EventId> {
        let global = self.global.read().unwrap_or_else(|e| e.into_inner());
        global.first_event_for_sequence.get(key).copied()
    }

    /// Aggregate statistics across all shards.
    ///
    /// Acquires a read lock on every shard and the global state to compute
    /// total tracked events, committed count, and per-shard load.
    pub fn stats(&self) -> ShardedConsensusStats {
        let mut total_tracked = 0usize;
        let mut shard_loads = Vec::with_capacity(NUM_SHARDS);

        for shard_lock in &self.shards {
            let shard = shard_lock.read().unwrap_or_else(|e| e.into_inner());
            let count = shard.event_states.len();
            total_tracked += count;
            shard_loads.push(count);
        }

        let committed = self.committed_count();

        ShardedConsensusStats {
            total_tracked,
            committed,
            shard_loads,
        }
    }

    /// Remove Committed events whose assigned round is older than
    /// `current_round - threshold` from all shards.
    ///
    /// This prevents unbounded growth of the sharded state in long-running
    /// nodes. Committed events that are this old are no longer needed for
    /// consensus decisions.
    ///
    /// # Arguments
    ///
    /// * `threshold` — Number of rounds of committed state to retain.
    /// * `current_round` — The current maximum round across all nodes.
    ///
    /// # Returns
    ///
    /// The number of entries removed across all shards.
    pub fn cleanup_old_committed(&self, threshold: u64, current_round: u64) -> usize {
        if current_round <= threshold {
            return 0;
        }

        let cutoff_round = current_round - threshold;
        let mut total_removed = 0usize;

        for shard_lock in &self.shards {
            let mut shard = shard_lock.write().unwrap_or_else(|e| e.into_inner());
            let to_remove: Vec<EventId> = shard
                .event_states
                .iter()
                .filter(|(_, &state)| state == ConsensusState::Committed)
                .filter_map(|(id, _)| {
                    let round = shard.event_rounds.get(id).copied()?;
                    if round < cutoff_round {
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect();

            let removed = to_remove.len();
            for id in &to_remove {
                shard.event_states.remove(id);
                shard.event_rounds.remove(id);
                shard.fame_status.remove(id);
            }
            total_removed += removed;
        }

        if total_removed > 0 {
            tracing::debug!(
                removed = total_removed,
                current_round,
                cutoff_round,
                "cleaned up old committed events from sharded state"
            );
        }

        total_removed
    }

    /// Get the current maximum round across all nodes (global read lock).
    ///
    /// Returns `0` if no nodes have been seen.
    pub fn current_round(&self) -> u64 {
        let global = self.global.read().unwrap_or_else(|e| e.into_inner());
        global.node_info.values().map(|i| i.current_round).max().unwrap_or(0)
    }

    /// Convenience method: cleanup using default threshold and current round.
    ///
    /// Uses [`DEFAULT_COMMITTED_ROUND_THRESHOLD`] as the retention window.
    pub fn cleanup_old_committed_default(&self) -> usize {
        let current = self.current_round();
        self.cleanup_old_committed(DEFAULT_COMMITTED_ROUND_THRESHOLD, current)
    }
}

impl Default for ShardedConsensusState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn make_event_id(first_byte: u8) -> EventId {
        let mut id = [0u8; 32];
        id[0] = first_byte;
        id
    }

    fn make_node_id(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    // ── Basic functionality tests ────────────────────────────────────────

    #[test]
    fn test_shard_index_distribution() {
        // Different first bytes map to different shards
        assert_eq!(ShardedConsensusState::shard_index(&make_event_id(0)), 0);
        assert_eq!(ShardedConsensusState::shard_index(&make_event_id(1)), 1);
        assert_eq!(ShardedConsensusState::shard_index(&make_event_id(255)), 255);
    }

    #[test]
    fn test_insert_and_get_event_state() {
        let state = ShardedConsensusState::new();

        let event_id = make_event_id(42);
        assert!(state.get_event_state(&event_id).is_none());

        state.insert_event_state(event_id, ConsensusState::Pending);
        assert_eq!(state.get_event_state(&event_id), Some(ConsensusState::Pending));

        state.insert_event_state(event_id, ConsensusState::Committed);
        assert_eq!(state.get_event_state(&event_id), Some(ConsensusState::Committed));
    }

    #[test]
    fn test_insert_and_get_event_round() {
        let state = ShardedConsensusState::new();

        let event_id = make_event_id(42);
        assert!(state.get_event_round(&event_id).is_none());

        state.insert_event_round(event_id, 7);
        assert_eq!(state.get_event_round(&event_id), Some(7));
    }

    #[test]
    fn test_is_committed() {
        let state = ShardedConsensusState::new();
        let event_id = make_event_id(10);

        assert!(!state.is_committed(&event_id));

        state.insert_event_state(event_id, ConsensusState::Pending);
        assert!(!state.is_committed(&event_id));

        state.insert_event_state(event_id, ConsensusState::Committed);
        assert!(state.is_committed(&event_id));
    }

    #[test]
    fn test_witness_recording() {
        let state = ShardedConsensusState::new();

        let e1 = make_event_id(1);
        let e2 = make_event_id(2);

        state.record_witness(0, e1);
        state.record_witness(0, e2);
        state.record_witness(1, e1);

        let witnesses_0 = state.get_witnesses_for_round(0);
        assert_eq!(witnesses_0.len(), 2);
        assert!(witnesses_0.contains(&e1));
        assert!(witnesses_0.contains(&e2));

        let witnesses_1 = state.get_witnesses_for_round(1);
        assert_eq!(witnesses_1.len(), 1);
        assert!(witnesses_1.contains(&e1));

        let witnesses_99 = state.get_witnesses_for_round(99);
        assert!(witnesses_99.is_empty());
    }

    #[test]
    fn test_committed_count() {
        let state = ShardedConsensusState::new();
        assert_eq!(state.committed_count(), 0);

        state.increment_committed(5);
        assert_eq!(state.committed_count(), 5);

        state.increment_committed(3);
        assert_eq!(state.committed_count(), 8);
    }

    #[test]
    fn test_node_info_update() {
        let state = ShardedConsensusState::new();
        let node_id = make_node_id(1);

        assert_eq!(state.get_node_round(&node_id), 0);

        state.update_node_info(node_id, |info| {
            info.current_round = 5;
            info.events_created = 10;
        });

        assert_eq!(state.get_node_round(&node_id), 5);
    }

    #[test]
    fn test_equivocation_tracking() {
        let state = ShardedConsensusState::new();
        let node_id = make_node_id(1);
        let event_id_1 = make_event_id(10);
        let event_id_2 = make_event_id(20);

        let key = (node_id, 0);

        // First event for (node, seq=0)
        state.record_first_sequence(key, event_id_1);
        assert_eq!(state.get_first_sequence(&key), Some(event_id_1));

        // Second event for same (node, seq=0) — should NOT overwrite
        state.record_first_sequence(key, event_id_2);
        assert_eq!(state.get_first_sequence(&key), Some(event_id_1));
    }

    #[test]
    fn test_stats_aggregation() {
        let state = ShardedConsensusState::new();

        // Insert events into different shards
        for i in 0u8..10 {
            let event_id = make_event_id(i);
            state.insert_event_state(event_id, ConsensusState::Pending);
            state.insert_event_round(event_id, i as u64);
        }

        let stats = state.stats();
        assert_eq!(stats.total_tracked, 10);
        assert_eq!(stats.committed, 0);

        // Check that shard_loads has the right length and entries are correct
        assert_eq!(stats.shard_loads.len(), NUM_SHARDS);
        for i in 0..10usize {
            assert_eq!(stats.shard_loads[i], 1);
        }
    }

    #[test]
    fn test_fame_status() {
        let state = ShardedConsensusState::new();
        let event_id = make_event_id(42);

        assert!(state.get_fame_status(&event_id).is_none());

        state.insert_fame_status(event_id, true);
        assert_eq!(state.get_fame_status(&event_id), Some(true));

        state.insert_fame_status(event_id, false);
        assert_eq!(state.get_fame_status(&event_id), Some(false));
    }

    #[test]
    fn test_contains_event() {
        let state = ShardedConsensusState::new();
        let event_id = make_event_id(42);

        assert!(!state.contains_event(&event_id));

        state.insert_event_state(event_id, ConsensusState::Pending);
        assert!(state.contains_event(&event_id));
    }

    #[test]
    fn test_cleanup_old_committed() {
        let state = ShardedConsensusState::new();

        // Insert committed events at round 1
        for i in 0u8..5 {
            let event_id = make_event_id(i);
            state.insert_event_state(event_id, ConsensusState::Committed);
            state.insert_event_round(event_id, 1);
        }

        // Insert pending event at round 1 (should NOT be cleaned up)
        let pending_id = make_event_id(200);
        state.insert_event_state(pending_id, ConsensusState::Pending);
        state.insert_event_round(pending_id, 1);

        // Insert committed event at round 100 (should NOT be cleaned up)
        let recent_id = make_event_id(100);
        state.insert_event_state(recent_id, ConsensusState::Committed);
        state.insert_event_round(recent_id, 100);

        // Set up node info so current_round() returns 100
        let node_id = make_node_id(1);
        state.update_node_info(node_id, |info| {
            info.current_round = 100;
        });

        // Cleanup with threshold=50: events at round 1 < (100-50)=50 should be removed
        let removed = state.cleanup_old_committed(50, 100);
        assert_eq!(removed, 5);

        // The recent committed event should still be there
        assert!(state.is_committed(&recent_id));

        // The pending event at round 1 should still be there
        assert_eq!(state.get_event_state(&pending_id), Some(ConsensusState::Pending));

        // The old committed events should be gone
        let old_id = make_event_id(0);
        assert!(state.get_event_state(&old_id).is_none());
    }

    // ── Concurrent access tests ─────────────────────────────────────────

    #[test]
    fn test_concurrent_writes_no_data_loss() {
        let state = Arc::new(ShardedConsensusState::new());
        let num_threads = 8;
        let events_per_thread = 1000u64;

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let base = thread_id as u64 * events_per_thread;
                    for seq in 0..events_per_thread {
                        let mut event_id = [0u8; 32];
                        // Distribute events across shards
                        event_id[0] = ((base + seq) % 256) as u8;
                        event_id[1] = thread_id as u8;
                        event_id[2] = (seq >> 8) as u8;
                        event_id[3] = (seq & 0xFF) as u8;

                        state.insert_event_state(event_id, ConsensusState::Pending);
                        state.insert_event_round(event_id, seq);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("thread should not panic");
        }

        // Verify no data was lost
        let stats = state.stats();
        assert_eq!(
            stats.total_tracked,
            (num_threads * events_per_thread) as usize,
            "Expected {} tracked events, got {}",
            num_threads * events_per_thread,
            stats.total_tracked
        );
    }

    #[test]
    fn test_concurrent_read_write_consistency() {
        let state = Arc::new(ShardedConsensusState::new());

        // Pre-populate some events
        for i in 0u8..50 {
            let event_id = make_event_id(i);
            state.insert_event_state(event_id, ConsensusState::Pending);
            state.insert_event_round(event_id, i as u64);
        }

        let state_reader = Arc::clone(&state);
        let state_writer = Arc::clone(&state);

        // Reader thread: continuously reads events
        let reader = thread::spawn(move || {
            for i in 0u8..50 {
                let event_id = make_event_id(i);
                // Should always find the pre-populated events
                let round = state_reader.get_event_round(&event_id);
                assert!(round.is_some() || i >= 50, "Pre-populated event should be readable");
            }
        });

        // Writer thread: adds more events
        let writer = thread::spawn(move || {
            for i in 50u8..100 {
                let event_id = make_event_id(i);
                state_writer.insert_event_state(event_id, ConsensusState::Pending);
                state_writer.insert_event_round(event_id, i as u64);
            }
        });

        reader.join().expect("reader should not panic");
        writer.join().expect("writer should not panic");
    }

    #[test]
    fn test_shard_consistency_event_states_and_rounds() {
        let state = ShardedConsensusState::new();

        // For each event, both state and round should be in the same shard
        for i in 0u8..50 {
            let event_id = make_event_id(i);
            state.insert_event_state(event_id, ConsensusState::Witness { round: i as u64 });
            state.insert_event_round(event_id, i as u64);

            // Verify consistency
            let evt_state = state.get_event_state(&event_id);
            let evt_round = state.get_event_round(&event_id);
            assert!(evt_state.is_some(), "Event state should exist for shard {}", i);
            assert!(evt_round.is_some(), "Event round should exist for shard {}", i);

            if let Some(ConsensusState::Witness { round }) = evt_state {
                assert_eq!(round, i as u64);
            } else {
                panic!("Expected Witness state, got {:?}", evt_state);
            }
            assert_eq!(evt_round, Some(i as u64));
        }
    }

    #[test]
    fn test_concurrent_committed_count() {
        let state = Arc::new(ShardedConsensusState::new());
        let num_threads = 8;
        let increments_per_thread = 1000u64;

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    for _ in 0..increments_per_thread {
                        state.increment_committed(1);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("thread should not panic");
        }

        // The committed counter should reflect all increments
        // Note: Since we use a global write lock, increments are serialized,
        // so the count should be exact.
        let expected = num_threads * increments_per_thread;
        assert_eq!(state.committed_count(), expected);
    }

    #[test]
    fn test_concurrent_equivocation_tracking() {
        let state = Arc::new(ShardedConsensusState::new());
        let node_id = make_node_id(1);
        let num_threads = 8;

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let state = Arc::clone(&state);
                let node_id = node_id;
                thread::spawn(move || {
                    let mut event_id = [0u8; 32];
                    event_id[0] = thread_id as u8;
                    event_id[1] = 1;
                    state.record_first_sequence((node_id, 0), event_id);
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("thread should not panic");
        }

        // Exactly one event should win the race for (node_id, 0)
        let first = state.get_first_sequence(&(node_id, 0));
        assert!(first.is_some(), "First sequence should be recorded");
        // The winning event should be from one of the threads
        let first_id = first.expect("checked above");
        assert!((first_id[0] as usize) < num_threads);
    }

    // ── Property tests using proptest ───────────────────────────────────

    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        /// Strategy: generate a random EventId
        fn arb_event_id() -> impl Strategy<Value = EventId> {
            any::<[u8; 32]>()
        }

        proptest! {
            /// Property: shard_index always returns a valid index (0..256)
            #[test]
            fn proptest_shard_index_valid(event_id in arb_event_id()) {
                let idx = ShardedConsensusState::shard_index(&event_id);
                assert!(idx < 256, "shard_index returned {} which is >= 256", idx);
            }

            /// Property: get_event_state returns what was inserted
            #[test]
            fn proptest_insert_get_roundtrip(
                event_id in arb_event_id(),
                round in 0u64..1000
            ) {
                let state = ShardedConsensusState::new();
                state.insert_event_round(event_id, round);
                prop_assert_eq!(state.get_event_round(&event_id), Some(round));
            }

            /// Property: stats.total_tracked equals the number of unique events inserted
            #[test]
            fn proptest_stats_total_tracked(
                event_ids in prop::collection::hash_set(arb_event_id(), 1..100)
            ) {
                let state = ShardedConsensusState::new();
                for event_id in &event_ids {
                    state.insert_event_state(*event_id, ConsensusState::Pending);
                }
                let stats = state.stats();
                prop_assert_eq!(stats.total_tracked, event_ids.len());
            }

            /// Property: committed_count reflects all increments
            #[test]
            fn proptest_committed_count(increments in prop::collection::vec(1u64..100, 1..50)) {
                let state = ShardedConsensusState::new();
                let expected: u64 = increments.iter().sum();
                for inc in &increments {
                    state.increment_committed(*inc);
                }
                prop_assert_eq!(state.committed_count(), expected);
            }

            /// Property: first_sequence is idempotent (first writer wins)
            #[test]
            fn proptest_first_sequence_idempotent(
                node_byte in any::<u8>(),
                seq in 0u64..100,
                first_id in arb_event_id(),
                second_id in arb_event_id()
            ) {
                let state = ShardedConsensusState::new();
                let node_id = {
                    let mut n = [0u8; 32];
                    n[0] = node_byte;
                    n
                };
                let key = (node_id, seq);

                state.record_first_sequence(key, first_id);
                state.record_first_sequence(key, second_id);

                // First should always win
                prop_assert_eq!(state.get_first_sequence(&key), Some(first_id));
            }
        }
    }
}
