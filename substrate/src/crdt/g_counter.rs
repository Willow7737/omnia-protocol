//! Grow-Only Counter (G-Counter) CRDT
//!
//! A GCounter can only be incremented. It tracks the maximum value seen
//! from each node, and its total value is the sum of all node contributions.
//!
//! Properties:
//! - Monotonic: value never decreases
//! - Associative, Commutative, Idempotent (ACID merge semantics)
//! - Suitable for: view counts, like counts, any monotonic metric
//!
//! Limitations:
//! - Cannot decrement (use PN-Counter for that)
//! - Maximum value is u64::MAX per node

use super::CvRDT;
use crate::vector_clock::NodeId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// A Grow-Only Counter CRDT
///
/// Each node maintains its own increment count. The total is the sum
/// of all node counts. Merge takes the maximum per node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GCounter {
    /// Per-node increment counts
    counts: BTreeMap<NodeId, u64>,
}

impl GCounter {
    /// Create a new, empty G-Counter
    pub fn new() -> Self {
        Self {
            counts: BTreeMap::new(),
        }
    }

    /// Increment the counter for a specific node
    ///
    /// # Arguments
    /// * `node_id` - The node performing the increment
    /// * `amount` - Amount to increment by (default 1)
    ///
    /// # Panics
    /// Panics if increment would overflow u64
    pub fn increment(&mut self, node_id: NodeId, amount: u64) {
        let entry = self.counts.entry(node_id).or_insert(0);
        *entry = entry.checked_add(amount).expect("GCounter overflow");
    }

    /// Get the current total value (sum of all node counts)
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Get the count for a specific node
    pub fn node_value(&self, node_id: &NodeId) -> u64 {
        self.counts.get(node_id).copied().unwrap_or(0)
    }

    /// Get all node contributions
    pub fn contributions(&self) -> &BTreeMap<NodeId, u64> {
        &self.counts
    }

    /// Compute a state hash for verification
    pub fn state_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for (node, count) in &self.counts {
            hasher.update(node);
            hasher.update(count.to_le_bytes());
        }
        hasher.finalize().into()
    }

    /// Get the number of nodes that have contributed
    pub fn node_count(&self) -> usize {
        self.counts.len()
    }
}

impl CvRDT for GCounter {
    fn merge(&mut self, other: &Self) {
        for (node, &count) in &other.counts {
            let entry = self.counts.entry(*node).or_insert(0);
            *entry = (*entry).max(count);
        }
    }
}

impl Default for GCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    /// Strategy for generating arbitrary GCounter states.
    /// Produces a GCounter by applying a random sequence of increments.
    fn gcounter_strategy() -> impl Strategy<Value = GCounter> {
        prop::collection::vec((any::<u8>(), 1u64..1000), 0..20).prop_map(|increments| {
            let mut counter = GCounter::new();
            for (node_byte, amount) in increments {
                let mut node_id = [0u8; 32];
                node_id[0] = node_byte;
                counter.increment(node_id, amount);
            }
            counter
        })
    }

    #[test]
    fn test_basic_increment() {
        let mut counter = GCounter::new();
        let n1 = node(1);

        assert_eq!(counter.value(), 0);
        counter.increment(n1, 1);
        assert_eq!(counter.value(), 1);
        counter.increment(n1, 5);
        assert_eq!(counter.value(), 6);
    }

    #[test]
    fn test_multi_node() {
        let mut counter = GCounter::new();
        let n1 = node(1);
        let n2 = node(2);
        let n3 = node(3);

        counter.increment(n1, 10);
        counter.increment(n2, 20);
        counter.increment(n3, 30);

        assert_eq!(counter.value(), 60);
        assert_eq!(counter.node_value(&n1), 10);
        assert_eq!(counter.node_value(&n2), 20);
        assert_eq!(counter.node_value(&n3), 30);
    }

    #[test]
    fn test_merge() {
        let n1 = node(1);
        let n2 = node(2);
        let n3 = node(3);

        let mut counter_a = GCounter::new();
        counter_a.increment(n1, 5);
        counter_a.increment(n2, 10);

        let mut counter_b = GCounter::new();
        counter_b.increment(n2, 8); // lower than a's 10
        counter_b.increment(n3, 15);

        counter_a.merge(&counter_b);

        // n1: 5 (only in a)
        // n2: 10 (max of 10 and 8)
        // n3: 15 (only in b)
        assert_eq!(counter_a.value(), 30);
        assert_eq!(counter_a.node_value(&n1), 5);
        assert_eq!(counter_a.node_value(&n2), 10);
        assert_eq!(counter_a.node_value(&n3), 15);
    }

    #[test]
    fn test_merge_commutative() {
        let n1 = node(1);
        let n2 = node(2);

        let mut a = GCounter::new();
        a.increment(n1, 5);

        let mut b = GCounter::new();
        b.increment(n2, 10);

        let merged_ab = a.merged(&b);
        let merged_ba = b.merged(&a);

        assert_eq!(merged_ab, merged_ba);
    }

    #[test]
    fn test_merge_idempotent() {
        let n1 = node(1);

        let mut a = GCounter::new();
        a.increment(n1, 5);

        let merged = a.merged(&a);
        assert_eq!(a.value(), merged.value());
    }

    #[test]
    fn test_merge_associative() {
        let n1 = node(1);
        let n2 = node(2);
        let n3 = node(3);

        let mut a = GCounter::new();
        a.increment(n1, 1);

        let mut b = GCounter::new();
        b.increment(n2, 2);

        let mut c = GCounter::new();
        c.increment(n3, 3);

        // (a merge b) merge c == a merge (b merge c)
        let ab_c = a.merged(&b).merged(&c);
        let a_bc = a.merged(&b.merged(&c));

        assert_eq!(ab_c, a_bc);
    }

    #[test]
    fn test_monotonic() {
        let n1 = node(1);
        let mut counter = GCounter::new();

        let v1 = counter.value();
        counter.increment(n1, 1);
        let v2 = counter.value();
        counter.increment(n1, 1);
        let v3 = counter.value();

        assert!(v1 < v2);
        assert!(v2 < v3);
    }

    #[test]
    fn test_state_hash() {
        let n1 = node(1);
        let mut counter = GCounter::new();
        let hash1 = counter.state_hash();
        counter.increment(n1, 1);
        let hash2 = counter.state_hash();

        assert_ne!(hash1, hash2);

        // Same state -> same hash
        let mut counter2 = GCounter::new();
        counter2.increment(n1, 1);
        assert_eq!(counter.state_hash(), counter2.state_hash());
    }

    // ── Property-based tests (proptest) ──────────────────────────────

    proptest! {
        /// For any two GCounters a, b: merge(a, b) == merge(b, a)
        #[test]
        fn proptest_merge_commutative(
            a in gcounter_strategy(),
            b in gcounter_strategy()
        ) {
            let merged_ab = a.merged(&b);
            let merged_ba = b.merged(&a);
            prop_assert_eq!(merged_ab, merged_ba);
        }

        /// For any GCounter a: merge(a, a) == a
        #[test]
        fn proptest_merge_idempotent(a in gcounter_strategy()) {
            let merged = a.merged(&a);
            prop_assert_eq!(merged, a);
        }

        /// For any three GCounters a, b, c:
        /// merge(merge(a, b), c) == merge(a, merge(b, c))
        #[test]
        fn proptest_merge_associative(
            a in gcounter_strategy(),
            b in gcounter_strategy(),
            c in gcounter_strategy()
        ) {
            let ab_then_c = a.merged(&b).merged(&c);
            let a_then_bc = a.merged(&b.merged(&c));
            prop_assert_eq!(ab_then_c, a_then_bc);
        }

        /// Value never decreases after increment or merge
        #[test]
        fn proptest_monotonic(
            a in gcounter_strategy(),
            b in gcounter_strategy(),
            node_byte in any::<u8>(),
            amount in 1u64..1000
        ) {
            // Increment never decreases value
            let mut node_id = [0u8; 32];
            node_id[0] = node_byte;
            let mut counter = a.clone();
            let before_inc = counter.value();
            counter.increment(node_id, amount);
            let after_inc = counter.value();
            prop_assert!(after_inc >= before_inc,
                "value decreased after increment: {} -> {}", before_inc, after_inc);

            // Merge never decreases value
            let val_a = a.value();
            let val_b = b.value();
            let merged = a.merged(&b);
            let val_merged = merged.value();
            prop_assert!(val_merged >= val_a,
                "merged value {} < a value {}", val_merged, val_a);
            prop_assert!(val_merged >= val_b,
                "merged value {} < b value {}", val_merged, val_b);
        }
    }
}
