//! Mempool for pending events awaiting consensus.
//!
//! The mempool holds events that have been submitted but not yet included
//! in a leader's block proposal. It is bounded to prevent unbounded memory
//! growth under load.

use std::collections::{HashSet, VecDeque};

use omnia_primitives::{Event, EventId};

/// Error type for mempool operations.
#[derive(Debug, thiserror::Error)]
pub enum MempoolError {
    /// The mempool has reached its maximum capacity.
    #[error("mempool full: {current}/{max}")]
    Full {
        /// Current number of events.
        current: usize,
        /// Maximum capacity.
        max: usize,
    },
}

/// A bounded mempool for pending events.
///
/// Events are stored in FIFO order. When a leader produces a block,
/// it drains up to `max_block_events` from the front of the queue.
///
/// Internally maintains a `HashSet<EventId>` alongside the `VecDeque`
/// for O(1) membership testing by event ID.
#[derive(Debug)]
pub struct Mempool {
    events: VecDeque<Event>,
    /// Set for O(1) membership testing by event ID.
    event_set: HashSet<EventId>,
    max_size: usize,
}

impl Mempool {
    /// Create a new mempool with the given maximum capacity.
    pub fn new(max_size: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(max_size.min(1024)),
            event_set: HashSet::with_capacity(max_size.min(1024)),
            max_size,
        }
    }

    /// Insert an event into the mempool.
    ///
    /// Returns an error if the mempool is full or if the event is a duplicate.
    pub fn insert(&mut self, event: Event) -> Result<(), MempoolError> {
        // Duplicate check: reject events already in the mempool
        if self.contains(&event.id) {
            return Ok(());
        }
        if self.events.len() >= self.max_size {
            return Err(MempoolError::Full {
                current: self.events.len(),
                max: self.max_size,
            });
        }
        let event_id = event.id;
        self.events.push_back(event);
        self.event_set.insert(event_id);
        Ok(())
    }

    /// Drain up to `limit` events from the mempool for block production.
    pub fn drain_up_to(&mut self, limit: usize) -> Vec<Event> {
        let count = limit.min(self.events.len());
        let drained: Vec<Event> = self.events.drain(..count).collect();
        // Remove drained event IDs from the set
        for event in &drained {
            self.event_set.remove(&event.id);
        }
        drained
    }

    /// Get the number of events in the mempool.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if the mempool is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get the maximum capacity.
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Remove a specific event from the mempool by its ID.
    ///
    /// Returns `true` if the event was found and removed, `false` otherwise.
    /// This is used to avoid proposing events that have already been
    /// inserted into the graph by another path (e.g., via `submit_event`).
    pub fn remove_by_id(&mut self, event_id: &EventId) -> bool {
        if let Some(pos) = self.events.iter().position(|e| &e.id == event_id) {
            self.events.remove(pos);
            self.event_set.remove(event_id);
            true
        } else {
            false
        }
    }

    /// Check if the mempool contains an event with the given ID.
    pub fn contains(&self, event_id: &EventId) -> bool {
        self.event_set.contains(event_id)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_crypto::generate_keypair;
    use omnia_primitives::Event;
    use omnia_primitives::NodeId;

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    fn make_event(creator: u8, seq: u64, payload: Vec<u8>) -> Event {
        let keypair = generate_keypair();
        let node = test_node(creator);
        let vc = omnia_primitives::VectorClock::with_node(node, seq + 1);
        let mut event = Event::new(node, seq, vc, None, None, payload).expect("valid event");
        event.sign_with_keypair(&keypair).expect("signing");
        event
    }

    #[test]
    fn test_mempool_insert_and_drain() {
        let mut mempool = Mempool::new(100);

        let e1 = make_event(1, 0, vec![1]);
        let e2 = make_event(2, 0, vec![2]);
        let e1_id = e1.id;
        let e2_id = e2.id;

        mempool.insert(e1).unwrap();
        mempool.insert(e2).unwrap();
        assert_eq!(mempool.len(), 2);

        let drained = mempool.drain_up_to(10);
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].id, e1_id);
        assert_eq!(drained[1].id, e2_id);
        assert!(mempool.is_empty());
    }

    #[test]
    fn test_mempool_drain_up_to() {
        let mut mempool = Mempool::new(100);

        for i in 0..10 {
            mempool.insert(make_event(1, i, vec![i as u8])).unwrap();
        }
        assert_eq!(mempool.len(), 10);

        let drained = mempool.drain_up_to(5);
        assert_eq!(drained.len(), 5);
        assert_eq!(mempool.len(), 5);

        // Remaining events should still be in order
        let remaining = mempool.drain_up_to(10);
        assert_eq!(remaining.len(), 5);
        assert!(mempool.is_empty());
    }

    #[test]
    fn test_mempool_full() {
        let mut mempool = Mempool::new(2);

        mempool.insert(make_event(1, 0, vec![1])).unwrap();
        mempool.insert(make_event(2, 0, vec![2])).unwrap();

        let result = mempool.insert(make_event(3, 0, vec![3]));
        assert!(result.is_err());
        match result.unwrap_err() {
            MempoolError::Full { current, max } => {
                assert_eq!(current, 2);
                assert_eq!(max, 2);
            }
        }
        assert_eq!(mempool.len(), 2);
    }

    #[test]
    fn test_mempool_empty_drain() {
        let mut mempool = Mempool::new(100);
        assert!(mempool.drain_up_to(10).is_empty());
        assert!(mempool.is_empty());
    }

    #[test]
    fn test_mempool_remove_by_id() {
        let mut mempool = Mempool::new(100);

        let e1 = make_event(1, 0, vec![1]);
        let e2 = make_event(2, 0, vec![2]);
        let e3 = make_event(3, 0, vec![3]);
        let e2_id = e2.id;

        mempool.insert(e1).unwrap();
        mempool.insert(e2).unwrap();
        mempool.insert(e3).unwrap();
        assert_eq!(mempool.len(), 3);

        // Remove the middle event
        assert!(mempool.remove_by_id(&e2_id));
        assert_eq!(mempool.len(), 2);
        assert!(!mempool.contains(&e2_id));

        // Remove non-existent event
        let fake_id = [0u8; 32];
        assert!(!mempool.remove_by_id(&fake_id));
        assert_eq!(mempool.len(), 2);
    }

    #[test]
    fn test_mempool_contains() {
        let mut mempool = Mempool::new(100);

        let e1 = make_event(1, 0, vec![1]);
        let e1_id = e1.id;

        assert!(!mempool.contains(&e1_id));
        mempool.insert(e1).unwrap();
        assert!(mempool.contains(&e1_id));
    }

    #[test]
    fn test_mempool_max_size() {
        let mempool = Mempool::new(500);
        assert_eq!(mempool.max_size(), 500);
    }

    #[test]
    fn test_mempool_drain_more_than_available() {
        let mut mempool = Mempool::new(100);

        mempool.insert(make_event(1, 0, vec![1])).unwrap();
        mempool.insert(make_event(2, 0, vec![2])).unwrap();

        // Requesting more than available should return all available
        let drained = mempool.drain_up_to(1000);
        assert_eq!(drained.len(), 2);
        assert!(mempool.is_empty());
    }
}
