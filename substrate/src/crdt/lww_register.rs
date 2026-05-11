//! Last-Write-Wins Register (LWW-Register) CRDT
//!
//! A simple CRDT that stores a single value. When concurrent writes occur,
//! the one with the highest timestamp wins. If timestamps are equal,
//! a deterministic tiebreaker (lexicographic comparison of node IDs) is used.
//!
//! Properties:
//! - Simplest possible CRDT
//! - Always converges to a single value
//! - "Last write wins" may lose data on concurrent writes
//!
//! Use cases:
//! - Configuration values
//! - User preferences
//! - Cache entries
//! - Any scenario where overwriting is acceptable

use super::CvRDT;
use crate::vector_clock::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A Last-Write-Wins Register CRDT
///
/// Stores a single value along with metadata for conflict resolution:
/// - `timestamp`: Wall-clock time of the write (for ordering)
/// - `node_id`: The node that performed the write (for tiebreaking)
/// - `vector_clock`: Causal information about the write
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LwwRegister<T: Clone + Serialize> {
    /// The stored value (None if never set)
    value: Option<T>,
    /// Wall-clock timestamp of the write
    timestamp: u64,
    /// Node that performed the write
    node_id: NodeId,
    /// Vector clock at time of write
    vector_clock: VectorClock,
    /// Monotonic version counter
    version: u64,
}

impl<T: Clone + Serialize> LwwRegister<T> {
    /// Create a new, empty LWW-Register
    pub fn new(node_id: NodeId) -> Self {
        Self {
            value: None,
            timestamp: 0,
            node_id,
            vector_clock: VectorClock::new(),
            version: 0,
        }
    }

    /// Create a new register with a value
    pub fn with_value(node_id: NodeId, value: T) -> Self {
        let mut reg = Self::new(node_id);
        reg.set(value);
        reg
    }

    /// Set the value of the register
    pub fn set(&mut self, value: T) {
        self.value = Some(value);
        self.timestamp = current_timestamp();
        self.version += 1;
    }

    /// Set the value with explicit metadata (used during merge)
    pub fn set_with_meta(&mut self, value: T, timestamp: u64, node_id: NodeId, version: u64) {
        self.value = Some(value);
        self.timestamp = timestamp;
        self.node_id = node_id;
        self.version = version;
    }

    /// Get the current value
    pub fn get(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Check if the register has been set
    pub fn is_set(&self) -> bool {
        self.value.is_some()
    }

    /// Get the timestamp of the current value
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Get the node ID of the last writer
    pub fn writer(&self) -> NodeId {
        self.node_id
    }

    /// Get the version
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Clear the value
    pub fn clear(&mut self) {
        self.value = None;
        self.timestamp = current_timestamp();
        self.version += 1;
    }

    /// Compare two registers to determine which write wins
    ///
    /// Returns true if self should win over other
    fn should_win(&self, other: &Self) -> bool {
        // Higher version wins
        if self.version != other.version {
            return self.version > other.version;
        }

        // Higher timestamp wins
        if self.timestamp != other.timestamp {
            return self.timestamp > other.timestamp;
        }

        // Tiebreaker: lexicographically greater node_id wins
        // This is deterministic across all nodes
        self.node_id > other.node_id
    }

    /// Compute state hash
    pub fn state_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.node_id);
        hasher.update(&self.version.to_le_bytes());
        if let Some(ref val) = self.value {
            let bytes = serde_json::to_vec(val).unwrap_or_default();
            hasher.update(&bytes);
        }
        hasher.finalize().into()
    }

    /// Get the vector clock
    pub fn vector_clock(&self) -> &VectorClock {
        &self.vector_clock
    }

    /// Update the vector clock
    pub fn set_vector_clock(&mut self, vc: VectorClock) {
        self.vector_clock = vc;
    }
}

impl<T: Clone + Serialize> CvRDT for LwwRegister<T> {
    fn merge(&mut self, other: &Self) {
        // If other should win, take its value
        if other.should_win(self) {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
            self.node_id = other.node_id;
            self.version = other.version;
        }
        // Merge vector clocks for causal tracking
        self.vector_clock.merge(&other.vector_clock);
    }
}

impl<T: Clone + Serialize + Default> Default for LwwRegister<T> {
    fn default() -> Self {
        Self::new([0u8; 32])
    }
}

impl<T: Clone + Serialize + fmt::Display> fmt::Display for LwwRegister<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            Some(v) => write!(f, "LWW({})", v),
            None => write!(f, "LWW(<empty>)"),
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    #[test]
    fn test_basic_set_get() {
        let mut reg = LwwRegister::new(node(1));

        assert!(!reg.is_set());
        assert_eq!(reg.get(), None);

        reg.set("hello");
        assert!(reg.is_set());
        assert_eq!(reg.get(), Some(&"hello"));
    }

    #[test]
    fn test_merge_newer_wins() {
        let n1 = node(1);

        let mut reg_a = LwwRegister::with_value(n1, "old");
        // Simulate time passing
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut reg_b = LwwRegister::with_value(n1, "new");

        reg_a.merge(&reg_b);
        assert_eq!(reg_a.get(), Some(&"new"));
    }

    #[test]
    fn test_merge_commutative() {
        let n1 = node(1);
        let n2 = node(2);

        let mut a = LwwRegister::new(n1);
        a.set("A");
        a.timestamp = 100;
        a.version = 1;

        let mut b = LwwRegister::new(n2);
        b.set("B");
        b.timestamp = 200; // B is newer
        b.version = 1;

        let merged_ab = a.merged(&b);
        let merged_ba = b.merged(&a);

        assert_eq!(merged_ab.get(), merged_ba.get());
        assert_eq!(merged_ab.get(), Some(&"B"));
    }

    #[test]
    fn test_merge_idempotent() {
        let n1 = node(1);

        let mut a = LwwRegister::new(n1);
        a.set("value");
        a.timestamp = 100;

        let merged = a.merged(&a);
        assert_eq!(a.get(), merged.get());
    }

    #[test]
    fn test_tiebreak_by_node_id() {
        // Same timestamp, same version -> higher node_id wins
        let n1 = node(1);
        let n2 = node(2); // n2 > n1

        let mut a = LwwRegister::new(n1);
        a.set("from_n1");
        a.timestamp = 100;
        a.version = 1;

        let mut b = LwwRegister::new(n2);
        b.set("from_n2");
        b.timestamp = 100; // same timestamp
        b.version = 1; // same version

        let merged = a.merged(&b);
        assert_eq!(merged.get(), Some(&"from_n2")); // n2 wins
    }

    #[test]
    fn test_version_wins_over_timestamp() {
        // Even with older timestamp, higher version wins
        let n1 = node(1);

        let mut a = LwwRegister::new(n1);
        a.set("version2");
        a.timestamp = 200;
        a.version = 2;

        let mut b = LwwRegister::new(n1);
        b.set("version1_newer_time");
        b.timestamp = 300; // newer timestamp
        b.version = 1; // but older version

        let merged = a.merged(&b);
        assert_eq!(merged.get(), Some(&"version2")); // version wins
    }

    #[test]
    fn test_clear() {
        let mut reg = LwwRegister::with_value(node(1), "hello");
        assert!(reg.is_set());

        reg.clear();
        assert!(!reg.is_set());
        assert_eq!(reg.get(), None);
    }

    #[test]
    fn test_state_hash() {
        let mut reg = LwwRegister::new(node(1));
        let hash1 = reg.state_hash();

        reg.set("hello");
        let hash2 = reg.state_hash();

        assert_ne!(hash1, hash2);

        // Same state -> same hash
        let reg2 = LwwRegister::with_value(node(1), "hello");
        assert_eq!(reg.state_hash(), reg2.state_hash());
    }

    #[test]
    fn test_numeric_values() {
        let mut reg = LwwRegister::new(node(1));
        reg.set(42u64);
        assert_eq!(reg.get(), Some(&42));

        reg.set(100);
        assert_eq!(reg.get(), Some(&100));
    }
}
