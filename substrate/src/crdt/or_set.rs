//! Observed-Remove Set (OR-Set) CRDT
//!
//! An OR-Set allows elements to be added and removed while ensuring that
//! concurrent add and remove operations don't conflict incorrectly.
//!
//! How it works:
//! - Each addition assigns a unique token to the element
//! - Removal "observes" and removes all visible tokens at the time of removal
//! - If an add happens concurrently with a remove, the new token survives
//! - This means "add wins" over concurrent remove
//!
//! Properties:
//! - Elements can be added and removed
//! - Concurrent add/remove: add wins (element present)
//! - Associative, Commutative, Idempotent
//!
//! Use cases:
//! - Shopping carts (add/remove items)
//! - Social features (follow/unfollow)
//! - Access control lists

use super::CvRDT;
use crate::vector_clock::NodeId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;

/// Unique token for tracking add operations
/// Format: (adding_node_id, sequence_number)
pub type Token = (NodeId, u64);

/// Observed-Remove Set CRDT
///
/// Internally tracks two sets per element:
/// - `adds`: All tokens ever added for this element
/// - `removes`: All tokens that were observed at removal time
///
/// An element is present if `adds - removes` is non-empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrSet<T: Clone + Ord + Hash + Serialize> {
    /// For each element, track all add tokens
    adds: BTreeMap<T, BTreeSet<Token>>,
    /// For each element, track all observed tokens at time of removal
    removes: BTreeMap<T, BTreeSet<Token>>,
    /// Sequence counter for generating unique tokens
    sequence: u64,
}

impl<T: Clone + Ord + Hash + Serialize> OrSet<T> {
    /// Create a new, empty OR-Set
    pub fn new() -> Self {
        Self {
            adds: BTreeMap::new(),
            removes: BTreeMap::new(),
            sequence: 0,
        }
    }

    /// Add an element to the set
    ///
    /// Returns the token assigned to this addition (can be used for removal)
    pub fn add(&mut self, node_id: NodeId, element: T) -> Token {
        self.sequence += 1;
        let token = (node_id, self.sequence);

        self.adds.entry(element).or_default().insert(token);

        token
    }

    /// Remove an element from the set
    ///
    /// This removes all currently visible tokens for the element.
    /// Concurrent additions (with new tokens) will survive.
    pub fn remove(&mut self, element: &T) {
        if let Some(tokens) = self.adds.get(element) {
            let observed = tokens.clone();
            self.removes
                .entry(element.clone())
                .or_default()
                .extend(observed);
        }
    }

    /// Remove a specific token (precise removal)
    ///
    /// This is useful when you know the exact token to remove.
    pub fn remove_token(&mut self, element: &T, token: &Token) {
        self.removes
            .entry(element.clone())
            .or_default()
            .insert(*token);
    }

    /// Check if an element is in the set
    pub fn contains(&self, element: &T) -> bool {
        match (self.adds.get(element), self.removes.get(element)) {
            (Some(adds), Some(removes)) => {
                // Element is present if there are add tokens not in removes
                adds.difference(removes).next().is_some()
            }
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// Get all elements currently in the set
    pub fn elements(&self) -> Vec<T> {
        self.adds
            .iter()
            .filter(|(elem, adds)| match self.removes.get(elem) {
                Some(removes) => adds.difference(removes).next().is_some(),
                None => true,
            })
            .map(|(elem, _)| elem.clone())
            .collect()
    }

    /// Get the number of elements in the set
    pub fn len(&self) -> usize {
        self.elements().len()
    }

    /// Check if the set is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get all tokens for an element (both adds and removes)
    pub fn tokens(&self, element: &T) -> Option<&BTreeSet<Token>> {
        self.adds.get(element)
    }

    /// Compute state hash
    pub fn state_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for (elem, tokens) in &self.adds {
            let elem_bytes = serde_json::to_vec(elem).unwrap_or_default();
            hasher.update(&elem_bytes);
            for (node, seq) in tokens {
                hasher.update(node);
                hasher.update(&seq.to_le_bytes());
            }
        }
        hasher.finalize().into()
    }
}

impl<T: Clone + Ord + Hash + Serialize> CvRDT for OrSet<T> {
    fn merge(&mut self, other: &Self) {
        // Merge adds: union of all add tokens
        for (element, tokens) in &other.adds {
            self.adds
                .entry(element.clone())
                .or_default()
                .extend(tokens.iter().cloned());
        }

        // Merge removes: union of all remove observations
        for (element, tokens) in &other.removes {
            self.removes
                .entry(element.clone())
                .or_default()
                .extend(tokens.iter().cloned());
        }
    }
}

impl<T: Clone + Ord + Hash + Serialize> Default for OrSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    /// Strategy for generating arbitrary OrSet<u32> states.
    /// Produces an OrSet by applying a random sequence of add/remove operations.
    fn orset_strategy() -> impl Strategy<Value = OrSet<u32>> {
        prop::collection::vec((any::<u8>(), any::<bool>(), any::<u32>()), 0..20)
            .prop_map(|ops| {
                let mut set: OrSet<u32> = OrSet::new();
                for (node_byte, is_add, element) in ops {
                    let mut node_id = [0u8; 32];
                    node_id[0] = node_byte;
                    if is_add {
                        set.add(node_id, element);
                    } else {
                        set.remove(&element);
                    }
                }
                set
            })
    }

    /// Compare two OrSets by their observable elements (ignoring internal sequence counter).
    fn orset_elements_eq(a: &OrSet<u32>, b: &OrSet<u32>) -> bool {
        let mut elems_a = a.elements();
        let mut elems_b = b.elements();
        elems_a.sort();
        elems_b.sort();
        elems_a == elems_b
    }

    #[test]
    fn test_add_and_contains() {
        let mut set = OrSet::new();
        let n1 = node(1);

        assert!(!set.contains(&"hello"));
        set.add(n1, "hello");
        assert!(set.contains(&"hello"));
    }

    #[test]
    fn test_remove() {
        let mut set = OrSet::new();
        let n1 = node(1);

        set.add(n1, "hello");
        assert!(set.contains(&"hello"));

        set.remove(&"hello");
        assert!(!set.contains(&"hello"));
    }

    #[test]
    fn test_elements() {
        let mut set = OrSet::new();
        let n1 = node(1);

        set.add(n1, "a");
        set.add(n1, "b");
        set.add(n1, "c");

        let mut elems = set.elements();
        elems.sort();
        assert_eq!(elems, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_add_wins_concurrent_remove() {
        // This is the key OR-Set property:
        // If node A removes element while node B adds it concurrently,
        // the add wins (element remains in set)
        let n1 = node(1);
        let n2 = node(2);

        let mut set_a = OrSet::new();
        set_a.add(n1, "item");

        // Simulate: node B sees the add and removes it
        let mut set_b = set_a.clone();
        set_b.remove(&"item");

        // Meanwhile, node A adds it again concurrently
        set_a.add(n1, "item");

        // After merge, item should be present (add wins)
        set_a.merge(&set_b);
        assert!(set_a.contains(&"item"));
    }

    #[test]
    fn test_merge_commutative() {
        let n1 = node(1);
        let n2 = node(2);

        let mut a = OrSet::new();
        a.add(n1, "x");

        let mut b = OrSet::new();
        b.add(n2, "y");

        let merged_ab = a.merged(&b);
        let merged_ba = b.merged(&a);

        assert_eq!(merged_ab.elements().sort(), merged_ba.elements().sort());
    }

    #[test]
    fn test_merge_idempotent() {
        let n1 = node(1);

        let mut a = OrSet::new();
        a.add(n1, "x");
        a.add(n1, "y");

        let merged = a.merged(&a);
        assert_eq!(a.len(), merged.len());
        assert_eq!(a.elements().sort(), merged.elements().sort());
    }

    #[test]
    fn test_token_tracking() {
        let n1 = node(1);
        let mut set = OrSet::new();

        let token1 = set.add(n1, "item");
        let token2 = set.add(n1, "item");

        // Same element, different tokens
        assert_ne!(token1, token2);

        let tokens = set.tokens(&"item").unwrap();
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains(&token1));
        assert!(tokens.contains(&token2));

        // Remove specific token
        set.remove_token(&"item", &token1);
        // Still present because token2 exists
        assert!(set.contains(&"item"));

        set.remove_token(&"item", &token2);
        // Now fully removed
        assert!(!set.contains(&"item"));
    }

    #[test]
    fn test_state_hash() {
        let n1 = node(1);
        let mut set = OrSet::new();
        let hash1 = set.state_hash();

        set.add(n1, "hello");
        let hash2 = set.state_hash();

        assert_ne!(hash1, hash2);

        // Same state -> same hash
        let mut set2 = OrSet::new();
        set2.add(n1, "hello");
        assert_eq!(set.state_hash(), set2.state_hash());
    }

    #[test]
    fn test_concurrent_adds_same_element() {
        // Two nodes add the same element concurrently
        // Both additions should survive
        let n1 = node(1);
        let n2 = node(2);

        let mut set_a = OrSet::new();
        set_a.add(n1, "shared");

        let mut set_b = OrSet::new();
        set_b.add(n2, "shared");

        set_a.merge(&set_b);

        assert!(set_a.contains(&"shared"));
        // Should have 2 tokens for the same element
        assert_eq!(set_a.tokens(&"shared").unwrap().len(), 2);
    }

    // ── Property-based tests (proptest) ──────────────────────────────

    proptest! {
        /// For any two OrSets a, b: merge(a, b) and merge(b, a) produce
        /// the same observable elements (commutativity of observable state).
        #[test]
        fn proptest_merge_commutative(
            a in orset_strategy(),
            b in orset_strategy()
        ) {
            let merged_ab = a.merged(&b);
            let merged_ba = b.merged(&a);
            prop_assert!(orset_elements_eq(&merged_ab, &merged_ba),
                "merge(a,b) and merge(b,a) produced different element sets");
        }

        /// For any OrSet a: merge(a, a) == a (idempotency).
        /// Since both operands are identical, struct equality holds
        /// (same adds, removes, and sequence counter).
        #[test]
        fn proptest_merge_idempotent(a in orset_strategy()) {
            let merged = a.merged(&a);
            prop_assert_eq!(merged, a);
        }

        /// If an element is concurrently added and removed, add wins.
        /// This is the core add-wins semantics of the OR-Set.
        #[test]
        fn proptest_add_wins(
            node_a_byte in any::<u8>(),
            node_b_byte in any::<u8>(),
            element in any::<u32>()
        ) {
            // Ensure different nodes for concurrent operations
            prop_assume!(node_a_byte != node_b_byte);

            let mut node_a = [0u8; 32];
            node_a[0] = node_a_byte;
            let mut node_b = [0u8; 32];
            node_b[0] = node_b_byte;

            // Node A adds the element
            let mut set_a: OrSet<u32> = OrSet::new();
            set_a.add(node_a, element);

            // Node B has the same state and removes the element
            let mut set_b = set_a.clone();
            set_b.remove(&element);

            // Meanwhile, Node A adds it again concurrently (new token)
            set_a.add(node_a, element);

            // After merging in either direction, element should be present
            let merged_ab = set_a.merged(&set_b);
            let merged_ba = set_b.merged(&set_a);
            prop_assert!(merged_ab.contains(&element),
                "add-wins violated: element absent after merge(a,b)");
            prop_assert!(merged_ba.contains(&element),
                "add-wins violated: element absent after merge(b,a)");
        }
    }
}
