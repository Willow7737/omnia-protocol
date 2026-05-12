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
    IncompatibleNodes,
    #[error("Invalid node ID: {0}")]
    InvalidNodeId(String),
    #[error("Clock overflow detected for node {node:?}")]
    ClockOverflow { node: NodeId },
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
    /// This represents the combined knowledge of causality from both clocks
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
}
