//! CRDT implementations for Omnia Protocol

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Trait for state-based CRDTs
pub trait CvRDT {
    /// Merge another CRDT into this one
    fn merge(&mut self, other: &Self);
}

/// Grow-only Counter CRDT
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GCounter {
    /// Map of node_id -> count
    pub(crate) counts: BTreeMap<[u8; 32], u64>,
}

impl GCounter {
    /// Create a new GCounter
    pub fn new() -> Self {
        Self {
            counts: BTreeMap::new(),
        }
    }

    /// Increment the counter for a specific node
    pub fn increment(&mut self, node_id: [u8; 32], value: u64) {
        let entry = self.counts.entry(node_id).or_insert(0);
        *entry += value;
    }

    /// Get the total value of the counter
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
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

/// Last-Write-Wins Register
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LwwRegister<T> {
    /// The value stored in the register
    pub value: T,
    /// Timestamp for last-write-wins semantics
    pub timestamp: u64,
}

impl<T: Clone + PartialEq> LwwRegister<T> {
    /// Create a new LwwRegister
    pub fn new(value: T, timestamp: u64) -> Self {
        Self { value, timestamp }
    }

    /// Set the value if the timestamp is newer
    pub fn set(&mut self, value: T, timestamp: u64) {
        if timestamp >= self.timestamp {
            self.value = value;
            self.timestamp = timestamp;
        }
    }
}

impl<T: Clone + PartialEq> CvRDT for LwwRegister<T> {
    fn merge(&mut self, other: &Self) {
        if other.timestamp > self.timestamp {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
        }
    }
}

/// Observed-Remove Set (OR-Set) CRDT
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrSet<T: Eq + std::hash::Hash + Clone> {
    /// Elements with unique tags
    pub(crate) elements: HashMap<T, Vec<u64>>,
    /// Tombstones for removed elements
    pub(crate) tombstones: HashMap<T, Vec<u64>>,
}

impl<T: Eq + std::hash::Hash + Clone> OrSet<T> {
    /// Create a new OR-Set
    pub fn new() -> Self {
        Self {
            elements: HashMap::new(),
            tombstones: HashMap::new(),
        }
    }

    /// Add an element with a unique tag
    pub fn add(&mut self, element: T, tag: u64) {
        self.elements.entry(element).or_default().push(tag);
    }

    /// Remove an element (adds tags to tombstones)
    pub fn remove(&mut self, element: &T) {
        if let Some(tags) = self.elements.get(element) {
            let tombstone_tags: Vec<u64> = tags.clone();
            self.tombstones
                .entry(element.clone())
                .or_default()
                .extend(tombstone_tags);
        }
    }

    /// Check if an element is in the set
    pub fn contains(&self, element: &T) -> bool {
        match (self.elements.get(element), self.tombstones.get(element)) {
            (Some(added), Some(removed)) => {
                added.iter().any(|tag| !removed.contains(tag))
            }
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// Get all active elements
    pub fn values(&self) -> Vec<T> {
        self.elements
            .iter()
            .filter(|(elem, tags)| {
                match self.tombstones.get(elem) {
                    Some(removed) => tags.iter().any(|tag| !removed.contains(tag)),
                    None => true,
                }
            })
            .map(|(elem, _)| elem.clone())
            .collect()
    }
}

impl<T: Eq + std::hash::Hash + Clone> CvRDT for OrSet<T> {
    fn merge(&mut self, other: &Self) {
        for (elem, tags) in &other.elements {
            let entry = self.elements.entry(elem.clone()).or_default();
            for tag in tags {
                if !entry.contains(tag) {
                    entry.push(*tag);
                }
            }
        }

        for (elem, tags) in &other.tombstones {
            let entry = self.tombstones.entry(elem.clone()).or_default();
            for tag in tags {
                if !entry.contains(tag) {
                    entry.push(*tag);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gcounter_basic() {
        let mut g = GCounter::new();
        let node = [1u8; 32];
        g.increment(node, 5);
        assert_eq!(g.value(), 5);
    }

    #[test]
    fn test_gcounter_merge() {
        let mut g1 = GCounter::new();
        let mut g2 = GCounter::new();
        let node1 = [1u8; 32];
        let node2 = [2u8; 32];

        g1.increment(node1, 3);
        g2.increment(node2, 7);

        g1.merge(&g2);
        assert_eq!(g1.value(), 10);
    }

    #[test]
    fn test_lww_register() {
        let mut reg = LwwRegister::new("a", 1);
        reg.set("b", 2);
        assert_eq!(reg.value, "b");
    }

    #[test]
    fn test_or_set() {
        let mut set = OrSet::new();
        set.add("a", 1);
        set.add("a", 2);
        assert!(set.contains(&"a"));
        set.remove(&"a");
        assert!(!set.contains(&"a"));
    }
}
