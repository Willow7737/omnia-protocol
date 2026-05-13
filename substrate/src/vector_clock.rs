//! Vector Clock Implementation
//!
//! Vector clocks track causal relationships between events in a distributed system.
//! Each node maintains a map of node_id -> logical counter, allowing us to determine:
//! - happened_before: One event causally precedes another
//! - concurrent: Two events are independent (can be processed in parallel)
//! - merge: Combine two vector clocks to capture all known causality
//!
//! Based on Leslie Lamport's logical clocks, extended to vector form
//! by Colin Fidge and Friedemann Mattern (1988).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use thiserror::Error;

/// Unique identifier for a node in the network
pub type NodeId = [u8; 32];

/// A logical timestamp for a single node
pub type LogicalClock = u64;

/// Errors that can occur during vector clock operations
#[derive(Error, Debug, Clone, PartialEq)]
pub enum VectorClockError {
    #[error("Cannot compare vector clocks: node IDs are incompatible")]
    /// Node IDs are incompatible for comparison
    IncompatibleNodes,
    #[error("Invalid node ID: {0}")]
    /// The node ID is invalid
    InvalidNodeId(String),
    #[error("Clock overflow detected for node {node:?}")]
    /// A clock value overflowed
    ClockOverflow {
        /// The node whose clock overflowed
        node: NodeId,
    },
}

/// VectorClock tracks the logical time across all known nodes.
///
/// Each entry represents the highest event sequence number known
/// from a particular node. This enables partial ordering of events
/// without any centralized clock.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    /// Map of node_id -> highest known sequence number from that node
    clocks: BTreeMap<NodeId, LogicalClock>,
}

/// Result of comparing two vector clocks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalOrder {
    /// self happened before other (self < other)
    Before,
    /// self happened after other (self > other)
    After,
    /// self and other are concurrent (independent)
    Concurrent,
    /// self and other are identical
    Equal,
}

impl VectorClock {
    /// Create a new, empty vector clock
    pub fn new() -> Self {
        Self {
            clocks: BTreeMap::new(),
        }
    }

    /// Create a vector clock with a single node's initial timestamp
    pub fn with_node(node_id: NodeId, clock: LogicalClock) -> Self {
        let mut clocks = BTreeMap::new();
        clocks.insert(node_id, clock);
        Self { clocks }
    }

    /// Increment the clock for a specific node
    pub fn increment(&mut self, node_id: NodeId) -> Result<LogicalClock, VectorClockError> {
        let entry = self.clocks.entry(node_id).or_insert(0);
        *entry = entry
            .checked_add(1)
            .ok_or(VectorClockError::ClockOverflow { node: node_id })?;
        Ok(*entry)
    }

    /// Get the clock value for a specific node (0 if unknown)
    pub fn get(&self, node_id: &NodeId) -> LogicalClock {
        self.clocks.get(node_id).copied().unwrap_or(0)
    }

    /// Set the clock value for a specific node
    pub fn set(&mut self, node_id: NodeId, clock: LogicalClock) {
        self.clocks.insert(node_id, clock);
    }

    /// Check if all entries in self are <= corresponding entries in other
    fn all_less_equal(&self, other: &Self) -> bool {
        self.clocks
            .iter()
            .all(|(node, &clock)| clock <= other.get(node))
    }

    /// Compare this vector clock with another to determine causal ordering
    pub fn compare(&self, other: &Self) -> CausalOrder {
        let self_leq_other = self.all_less_equal(other);
        let other_leq_self = other.all_less_equal(self);

        match (self_leq_other, other_leq_self) {
            (true, true) => CausalOrder::Equal,
            (true, false) => CausalOrder::Before,
            (false, true) => CausalOrder::After,
            (false, false) => CausalOrder::Concurrent,
        }
    }

    /// Check if this event happened before another (self < other)
    pub fn happened_before(&self, other: &Self) -> bool {
        matches!(self.compare(other), CausalOrder::Before)
    }

    /// Check if this event happened after another (self > other)
    pub fn happened_after(&self, other: &Self) -> bool {
        matches!(self.compare(other), CausalOrder::After)
    }

    /// Check if two events are concurrent (independent, can be parallelized)
    pub fn concurrent(&self, other: &Self) -> bool {
        matches!(self.compare(other), CausalOrder::Concurrent)
    }

    /// Merge two vector clocks, taking the maximum of each entry
    /// This represents the combined knowledge of causality from both clocks.
    ///
    /// # Reconciliation Strategy
    ///
    /// After a network partition, two replicas may have divergent vector clocks.
    /// The `merge` operation reconciles them by computing the pointwise maximum:
    ///
    /// ```text
    /// merge(VC_a, VC_b)[i] = max(VC_a[i], VC_b[i])    ∀ i ∈ (dom(VC_a) ∪ dom(VC_b))
    /// ```
    ///
    /// This strategy guarantees:
    /// - **Convergence**: After both sides merge, they arrive at the same clock.
    ///   Formally: `merge(A, B) = merge(B, A)` (commutativity).
    /// - **Causal preservation**: If `A[i] ≤ B[i]` for all i, then `merge(A, B) = B`.
    ///   The merge never loses causal information — it monotonically advances.
    /// - **Partition recovery**: Suppose nodes {N₁, N₂} are partitioned from {N₃, N₄}.
    ///   During the partition, each side increments only its own entries. After
    ///   reconnection and mutual merge, the resulting clock contains the maximum
    ///   of all counters, correctly reflecting that events from both partitions
    ///   have been observed.
    ///
    /// The resulting merged clock is a superset (in the ≤ sense) of both inputs,
    /// meaning `A ≤ merge(A, B)` and `B ≤ merge(A, B)`.
    pub fn merge(&mut self, other: &Self) {
        for (node, &clock) in other.clocks.iter() {
            let entry = self.clocks.entry(*node).or_insert(0);
            *entry = (*entry).max(clock);
        }
    }

    /// Create a new vector clock that is the merge of self and other
    pub fn merged(&self, other: &Self) -> Self {
        let mut result = self.clone();
        result.merge(other);
        result
    }

    /// Return the number of known nodes in this vector clock
    pub fn node_count(&self) -> usize {
        self.clocks.len()
    }

    /// Check if this vector clock is empty (no entries)
    pub fn is_empty(&self) -> bool {
        self.clocks.is_empty()
    }

    /// Get all node IDs in this vector clock
    pub fn nodes(&self) -> impl Iterator<Item = &NodeId> {
        self.clocks.keys()
    }

    /// Get the sum of all clock values (used for rough comparison)
    pub fn total(&self) -> u128 {
        self.clocks.values().map(|&v| v as u128).sum()
    }

    /// Prune entries that are below a threshold to save memory
    /// Returns number of entries pruned
    pub fn prune_below(&mut self, threshold: LogicalClock) -> usize {
        let before = self.clocks.len();
        self.clocks.retain(|_, &mut v| v >= threshold);
        before - self.clocks.len()
    }

    /// Serialize to bytes for network transmission
    pub fn to_bytes(&self) -> Vec<u8> {
        // Simple binary format: [count:u32][(node_id:32bytes, clock:8bytes)...]
        let mut bytes = Vec::with_capacity(4 + self.clocks.len() * 40);
        bytes.extend_from_slice(&(self.clocks.len() as u32).to_le_bytes());
        for (node, clock) in &self.clocks {
            bytes.extend_from_slice(node);
            bytes.extend_from_slice(&clock.to_le_bytes());
        }
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, VectorClockError> {
        if bytes.len() < 4 {
            return Err(VectorClockError::InvalidNodeId(
                "insufficient bytes".to_string(),
            ));
        }
        let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let mut clocks = BTreeMap::new();
        let mut offset = 4;
        for _ in 0..count {
            if offset + 40 > bytes.len() {
                return Err(VectorClockError::InvalidNodeId(
                    "truncated entry".to_string(),
                ));
            }
            let mut node_id = [0u8; 32];
            node_id.copy_from_slice(&bytes[offset..offset + 32]);
            let clock = u64::from_le_bytes(bytes[offset + 32..offset + 40].try_into().unwrap());
            clocks.insert(node_id, clock);
            offset += 40;
        }
        Ok(Self { clocks })
    }
}

impl fmt::Display for VectorClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VC[")?;
        let mut first = true;
        for (node, clock) in &self.clocks {
            if !first {
                write!(f, ", ")?;
            }
            let short_id = hex::encode(&node[..4]);
            write!(f, "{}:{}", short_id, clock)?;
            first = false;
        }
        write!(f, "]")
    }
}

impl fmt::Display for CausalOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CausalOrder::Before => write!(f, "<"),
            CausalOrder::After => write!(f, ">"),
            CausalOrder::Concurrent => write!(f, "||"),
            CausalOrder::Equal => write!(f, "=="),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nid(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    #[test]
    fn test_increment() {
        let mut vc = VectorClock::new();
        let n1 = nid(1);

        assert_eq!(vc.increment(n1).unwrap(), 1);
        assert_eq!(vc.increment(n1).unwrap(), 2);
        assert_eq!(vc.get(&n1), 2);
    }

    #[test]
    fn test_happened_before() {
        let n1 = nid(1);
        let n2 = nid(2);

        // Event at n1: [1, 0]
        let mut vc_a = VectorClock::new();
        vc_a.set(n1, 1);

        // Event at n1 after receiving from n2: [2, 1]
        let mut vc_b = VectorClock::new();
        vc_b.set(n1, 2);
        vc_b.set(n2, 1);

        // vc_a -> vc_b (a happened before b)
        assert!(vc_a.happened_before(&vc_b));
        assert!(vc_b.happened_after(&vc_a));
        assert!(!vc_a.concurrent(&vc_b));
    }

    #[test]
    fn test_concurrent_events() {
        let n1 = nid(1);
        let n2 = nid(2);

        // n1 creates event without knowing n2's events: [1, 0]
        let mut vc_a = VectorClock::new();
        vc_a.set(n1, 1);

        // n2 creates event without knowing n1's events: [0, 1]
        let mut vc_b = VectorClock::new();
        vc_b.set(n2, 1);

        // These are concurrent (independent)
        assert!(vc_a.concurrent(&vc_b));
        assert!(vc_b.concurrent(&vc_a));
        assert!(!vc_a.happened_before(&vc_b));
        assert!(!vc_b.happened_before(&vc_a));
    }

    #[test]
    fn test_merge() {
        let n1 = nid(1);
        let n2 = nid(2);
        let n3 = nid(3);

        let mut vc_a = VectorClock::new();
        vc_a.set(n1, 3);
        vc_a.set(n2, 1);

        let mut vc_b = VectorClock::new();
        vc_b.set(n2, 2);
        vc_b.set(n3, 5);

        // Merge: should get {n1:3, n2:2, n3:5}
        let merged = vc_a.merged(&vc_b);
        assert_eq!(merged.get(&n1), 3);
        assert_eq!(merged.get(&n2), 2); // max of 1 and 2
        assert_eq!(merged.get(&n3), 5);
    }

    #[test]
    fn test_equality() {
        let n1 = nid(1);
        let mut vc_a = VectorClock::new();
        vc_a.set(n1, 5);

        let mut vc_b = VectorClock::new();
        vc_b.set(n1, 5);

        assert_eq!(vc_a.compare(&vc_b), CausalOrder::Equal);
    }

    #[test]
    fn test_empty_clock_properties() {
        let empty = VectorClock::new();
        let n1 = nid(1);
        let mut vc = VectorClock::new();
        vc.set(n1, 1);

        // Empty clock is "before" any non-empty clock
        assert!(empty.happened_before(&vc));
        assert!(vc.happened_after(&empty));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let n1 = nid(1);
        let n2 = nid(2);
        let mut vc = VectorClock::new();
        vc.set(n1, 42);
        vc.set(n2, 100);

        let bytes = vc.to_bytes();
        let restored = VectorClock::from_bytes(&bytes).unwrap();
        assert_eq!(vc, restored);
    }

    #[test]
    fn test_display() {
        let n1 = nid(1);
        let mut vc = VectorClock::new();
        vc.set(n1, 5);
        let s = format!("{}", vc);
        assert!(s.contains("01"));
        assert!(s.contains(":5"));
    }

    #[test]
    fn test_prune() {
        let n1 = nid(1);
        let n2 = nid(2);
        let n3 = nid(3);
        let mut vc = VectorClock::new();
        vc.set(n1, 10);
        vc.set(n2, 5);
        vc.set(n3, 1);

        let pruned = vc.prune_below(5);
        assert_eq!(pruned, 1); // n3 removed
        assert_eq!(vc.get(&n1), 10);
        assert_eq!(vc.get(&n2), 5);
        assert_eq!(vc.get(&n3), 0); // gone
    }

    // ── Partition reconciliation tests (Sprint 1, Task 1.3) ────────────

    /// Simulate a network partition where two groups of nodes operate
    /// independently, then reconcile via merge. Both sides must converge
    /// to the same clock state.
    #[test]
    fn test_merge_converges_after_partition() {
        let n1 = nid(1);
        let n2 = nid(2);
        let n3 = nid(3);
        let n4 = nid(4);

        // Partition A: nodes {n1, n2} operate independently
        let mut vc_a = VectorClock::new();
        vc_a.set(n1, 5);
        vc_a.set(n2, 3);

        // Partition B: nodes {n3, n4} operate independently
        let mut vc_b = VectorClock::new();
        vc_b.set(n3, 7);
        vc_b.set(n4, 2);

        // After partition heals, both sides merge
        let merged_ab = vc_a.merged(&vc_b);
        let merged_ba = vc_b.merged(&vc_a);

        // Both must converge to the same state
        assert_eq!(merged_ab, merged_ba);
        assert_eq!(merged_ab.get(&n1), 5);
        assert_eq!(merged_ab.get(&n2), 3);
        assert_eq!(merged_ab.get(&n3), 7);
        assert_eq!(merged_ab.get(&n4), 2);
    }

    /// Test that CausalOrder is deterministic after merge.
    /// Two clocks that have been merged from the same set of partitions
    /// must always produce the same comparison result against any third clock.
    #[test]
    fn test_causal_order_deterministic_post_merge() {
        let n1 = nid(1);
        let n2 = nid(2);
        let n3 = nid(3);

        // Two replicas start from the same initial state
        let mut replica_a = VectorClock::new();
        replica_a.set(n1, 3);
        replica_a.set(n2, 2);

        let mut replica_b = VectorClock::new();
        replica_b.set(n1, 3);
        replica_b.set(n2, 2);

        // Partition: replica_a gets an update from n1
        replica_a.increment(n1).unwrap(); // n1 -> 4

        // Partition: replica_b gets an update from n2
        replica_b.increment(n2).unwrap(); // n2 -> 3

        // Now they are concurrent
        assert_eq!(replica_a.compare(&replica_b), CausalOrder::Concurrent);

        // Merge both
        let merged_a = replica_a.merged(&replica_b);
        let merged_b = replica_b.merged(&replica_a);

        // Both merges must be equal (deterministic)
        assert_eq!(merged_a, merged_b);

        // Create a third clock that happened before the partition
        let mut vc_before = VectorClock::new();
        vc_before.set(n1, 3);
        vc_before.set(n2, 2);

        // Both merged clocks must agree on the causal relationship with vc_before
        assert_eq!(merged_a.compare(&vc_before), CausalOrder::After);
        assert_eq!(merged_b.compare(&vc_before), CausalOrder::After);
    }

    /// Test that `happened_before` relationships are preserved after merge.
    /// If event A happened before event B, then after merging any additional
    /// clock information, A still happened before B.
    #[test]
    fn test_happened_before_preserved_after_merge() {
        let n1 = nid(1);
        let n2 = nid(2);
        let n3 = nid(3);

        // Event A: n1 = 2
        let mut vc_a = VectorClock::new();
        vc_a.set(n1, 2);

        // Event B: n1 = 2, n2 = 3 (A happened before B)
        let mut vc_b = VectorClock::new();
        vc_b.set(n1, 2);
        vc_b.set(n2, 3);

        // Verify A happened before B
        assert!(vc_a.happened_before(&vc_b));

        // Now simulate merge with a third partition's clock
        let mut vc_partition = VectorClock::new();
        vc_partition.set(n3, 10);

        let merged_a = vc_a.merged(&vc_partition);
        let merged_b = vc_b.merged(&vc_partition);

        // The happened_before relationship must still hold after merge
        assert!(
            merged_a.happened_before(&merged_b),
            "happened_before not preserved after merge: {:?} vs {:?}",
            merged_a,
            merged_b
        );
    }

    /// Test a multi-round partition reconciliation scenario.
    /// Simulates multiple partition/heal cycles where nodes progressively
    /// learn about each other's events.
    #[test]
    fn test_multi_round_partition_reconciliation() {
        let n1 = nid(1);
        let n2 = nid(2);
        let n3 = nid(3);

        // Round 1: Initial partition between {n1} and {n2, n3}
        let mut vc1 = VectorClock::with_node(n1, 5);
        let mut vc2 = VectorClock::new();
        vc2.set(n2, 3);
        vc2.set(n3, 2);

        // Reconcile round 1
        vc1.merge(&vc2);
        assert_eq!(vc1.get(&n1), 5);
        assert_eq!(vc1.get(&n2), 3);
        assert_eq!(vc1.get(&n3), 2);

        // Round 2: Another partition — n1 and n3 advance independently
        vc1.increment(n1).unwrap(); // n1 -> 6
        let mut vc3 = VectorClock::new();
        vc3.set(n3, 7); // n3 advanced further

        // Reconcile round 2
        vc1.merge(&vc3);
        assert_eq!(vc1.get(&n1), 6);
        assert_eq!(vc1.get(&n2), 3);
        assert_eq!(vc1.get(&n3), 7); // updated to max

        // Verify convergence: if vc2 also merged with vc3, they should match vc1
        vc2.merge(&vc3);
        // But vc2 is missing n1's updates from round 2
        assert_eq!(vc2.get(&n1), 0); // vc2 doesn't know about n1
                                     // After final reconciliation between vc1 and vc2
        vc2.merge(&vc1);
        assert_eq!(vc2.get(&n1), 6);
        assert_eq!(vc2.get(&n2), 3);
        assert_eq!(vc2.get(&n3), 7);
        // Now both have converged
        assert_eq!(vc1, vc2);
    }
}
