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
use omnia_primitives::NodeId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

/// Errors that can occur during CRDT operations.
#[derive(Debug, Error)]
pub enum CrdtError {
    /// Operation would cause an overflow.
    #[error("CRDT overflow")]
    Overflow,
}

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
    /// # Errors
    /// Returns `CrdtError::Overflow` if the increment would overflow u64.
    pub fn increment(&mut self, node_id: NodeId, amount: u64) -> Result<(), CrdtError> {
        let entry = self.counts.entry(node_id).or_insert(0);
        *entry = entry.checked_add(amount).ok_or(CrdtError::Overflow)?;
        Ok(())
    }

    /// Get the current total value (sum of all node counts).
    ///
    /// Saturates at `u64::MAX` if the sum overflows, preserving monotonicity.
    /// When saturation occurs, a warning is logged because the true value
    /// is unknown and subsequent increments will not be reflected.
    pub fn value(&self) -> u64 {
        self.counts
            .values()
            .copied()
            .try_fold(0u64, |acc, v| acc.checked_add(v))
            .unwrap_or_else(|| {
                tracing::warn!(
                    "GCounter value() saturated at u64::MAX — true sum overflows. \
                     Subsequent increments will not be reflected in value()."
                );
                u64::MAX
            })
    }

    /// Get the current total value, returning an error if the sum overflows.
    pub fn value_checked(&self) -> Result<u64, CrdtError> {
        self.counts
            .values()
            .copied()
            .try_fold(0u64, |acc, v| acc.checked_add(v).ok_or(CrdtError::Overflow))
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
                let _ = counter.increment(node_id, amount);
            }
            counter
        })
    }

    #[test]
    fn test_basic_increment() {
        let mut counter = GCounter::new();
        let n1 = node(1);

        assert_eq!(counter.value(), 0);
        counter.increment(n1, 1).unwrap();
        assert_eq!(counter.value(), 1);
        counter.increment(n1, 5).unwrap();
        assert_eq!(counter.value(), 6);
    }

    #[test]
    fn test_multi_node() {
        let mut counter = GCounter::new();
        let n1 = node(1);
        let n2 = node(2);
        let n3 = node(3);

        counter.increment(n1, 10).unwrap();
        counter.increment(n2, 20).unwrap();
        counter.increment(n3, 30).unwrap();

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
        counter_a.increment(n1, 5).unwrap();
        counter_a.increment(n2, 10).unwrap();

        let mut counter_b = GCounter::new();
        counter_b.increment(n2, 8).unwrap(); // lower than a's 10
        counter_b.increment(n3, 15).unwrap();

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
        a.increment(n1, 5).unwrap();

        let mut b = GCounter::new();
        b.increment(n2, 10).unwrap();

        let merged_ab = a.merged(&b);
        let merged_ba = b.merged(&a);

        assert_eq!(merged_ab, merged_ba);
    }

    #[test]
    fn test_merge_idempotent() {
        let n1 = node(1);

        let mut a = GCounter::new();
        a.increment(n1, 5).unwrap();

        let merged = a.merged(&a);
        assert_eq!(a.value(), merged.value());
    }

    #[test]
    fn test_merge_associative() {
        let n1 = node(1);
        let n2 = node(2);
        let n3 = node(3);

        let mut a = GCounter::new();
        a.increment(n1, 1).unwrap();

        let mut b = GCounter::new();
        b.increment(n2, 2).unwrap();

        let mut c = GCounter::new();
        c.increment(n3, 3).unwrap();

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
        counter.increment(n1, 1).unwrap();
        let v2 = counter.value();
        counter.increment(n1, 1).unwrap();
        let v3 = counter.value();

        assert!(v1 < v2);
        assert!(v2 < v3);
    }

    #[test]
    fn test_state_hash() {
        let n1 = node(1);
        let mut counter = GCounter::new();
        let hash1 = counter.state_hash();
        counter.increment(n1, 1).unwrap();
        let hash2 = counter.state_hash();

        assert_ne!(hash1, hash2);

        // Same state -> same hash
        let mut counter2 = GCounter::new();
        counter2.increment(n1, 1).unwrap();
        assert_eq!(counter.state_hash(), counter2.state_hash());
    }

    #[test]
    fn test_value_saturates_on_overflow() {
        let mut counter = GCounter::new();
        let n1 = node(1);
        let n2 = node(2);
        counter.increment(n1, u64::MAX).unwrap();
        counter.increment(n2, 1).unwrap();
        // value() should saturate at u64::MAX rather than wrapping
        assert_eq!(counter.value(), u64::MAX);
        // value_checked() should return Overflow error
        assert!(counter.value_checked().is_err());
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
            let _ = counter.increment(node_id, amount);
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
