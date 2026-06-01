//! Priority gossip queue for finality-critical events
//!
//! Events related to finality (witness events, fame votes) are prioritized
//! over regular transaction events. This ensures that consensus-critical
//! messages propagate faster, reducing finality latency.
//!
//! # Priority Levels
//!
//! - **Critical**: Witness events and other consensus-critical messages
//!   that directly affect finality. These jump to the front of the queue.
//! - **High**: Fame determination events and round-finalization messages.
//! - **Normal**: Regular transaction events and state updates.
//! - **Low**: Old retransmissions and bulk sync messages.
//!
//! # Bounded Capacity
//!
//! Each priority level has a bounded capacity. When a level is full,
//! new events at that level are dropped (oldest first). This prevents
//! any single priority level from consuming unbounded memory.

use omnia_primitives::EventId;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

/// Priority levels for gossip events.
///
/// Higher priority events are dequeued before lower priority ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GossipPriority {
    /// Lowest priority — old retransmissions, bulk sync.
    Low = 0,
    /// Normal priority — regular transaction events.
    Normal = 1,
    /// High priority — fame determination, round-finalization.
    High = 2,
    /// Critical priority — witness events, finality-critical messages.
    Critical = 3,
}

impl GossipPriority {
    /// All priority levels in descending order (Critical first).
    pub fn all_descending() -> &'static [GossipPriority] {
        &[
            GossipPriority::Critical,
            GossipPriority::High,
            GossipPriority::Normal,
            GossipPriority::Low,
        ]
    }

    /// Determine the priority for an event based on its characteristics.
    ///
    /// # Arguments
    ///
    /// * `is_witness` — Whether the event is a witness event.
    /// * `is_fame_determination` — Whether the event is a fame determination event.
    /// * `is_retransmission` — Whether the event is an old retransmission.
    pub fn classify(is_witness: bool, is_fame_determination: bool, is_retransmission: bool) -> Self {
        if is_witness {
            GossipPriority::Critical
        } else if is_fame_determination {
            GossipPriority::High
        } else if is_retransmission {
            GossipPriority::Low
        } else {
            GossipPriority::Normal
        }
    }
}

impl std::fmt::Display for GossipPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GossipPriority::Critical => write!(f, "CRITICAL"),
            GossipPriority::High => write!(f, "HIGH"),
            GossipPriority::Normal => write!(f, "NORMAL"),
            GossipPriority::Low => write!(f, "LOW"),
        }
    }
}

/// Configuration for the priority gossip queue.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriorityQueueConfig {
    /// Maximum events at Critical priority.
    pub max_critical: usize,
    /// Maximum events at High priority.
    pub max_high: usize,
    /// Maximum events at Normal priority.
    pub max_normal: usize,
    /// Maximum events at Low priority.
    pub max_low: usize,
}

impl Default for PriorityQueueConfig {
    fn default() -> Self {
        Self {
            max_critical: 1000,
            max_high: 5000,
            max_normal: 10000,
            max_low: 5000,
        }
    }
}

impl PriorityQueueConfig {
    /// Get the maximum capacity for a given priority level.
    pub fn max_for_priority(&self, priority: GossipPriority) -> usize {
        match priority {
            GossipPriority::Critical => self.max_critical,
            GossipPriority::High => self.max_high,
            GossipPriority::Normal => self.max_normal,
            GossipPriority::Low => self.max_low,
        }
    }
}

/// Priority gossip queue with bounded size per priority level.
///
/// Events are dequeued in strict priority order: all Critical events
/// are processed before any High events, all High before Normal, etc.
///
/// Within the same priority level, events are processed in FIFO order.
///
/// # Capacity Management
///
/// Each priority level has a bounded capacity. When a level is full,
/// the oldest event at that level is dropped to make room for the new one.
#[derive(Clone, Debug)]
pub struct PriorityGossipQueue {
    /// Queues per priority level, indexed by priority value.
    queues: [VecDeque<EventId>; 4],
    /// Set of event IDs currently in any queue, for O(1) contains checks.
    seen: HashSet<EventId>,
    /// Configuration for capacity limits.
    config: PriorityQueueConfig,
    /// Total number of events ever enqueued (for statistics).
    total_enqueued: u64,
    /// Total number of events dropped due to capacity.
    total_dropped: u64,
}

impl PriorityGossipQueue {
    /// Create a new priority gossip queue with the given configuration.
    pub fn new(config: PriorityQueueConfig) -> Self {
        Self {
            queues: [
                VecDeque::new(), // Low
                VecDeque::new(), // Normal
                VecDeque::new(), // High
                VecDeque::new(), // Critical
            ],
            seen: HashSet::new(),
            config,
            total_enqueued: 0,
            total_dropped: 0,
        }
    }

    /// Create a new priority gossip queue with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(PriorityQueueConfig::default())
    }

    /// Enqueue an event at the given priority level.
    ///
    /// If the queue for this priority level is at capacity, the oldest
    /// event is dropped to make room.
    pub fn enqueue(&mut self, event_id: EventId, priority: GossipPriority) {
        let queue = &mut self.queues[priority as usize];
        let max = self.config.max_for_priority(priority);

        // If at capacity, drop the oldest event
        if queue.len() >= max {
            if let Some(dropped) = queue.pop_front() {
                self.seen.remove(&dropped);
                self.total_dropped += 1;
            }
        }

        self.seen.insert(event_id);
        queue.push_back(event_id);
        self.total_enqueued += 1;
    }

    /// Dequeue the highest-priority event.
    ///
    /// Returns `None` if the queue is empty.
    pub fn dequeue(&mut self) -> Option<EventId> {
        // Check queues in priority order (Critical first)
        for priority in GossipPriority::all_descending() {
            let queue = &mut self.queues[*priority as usize];
            if let Some(event_id) = queue.pop_front() {
                self.seen.remove(&event_id);
                return Some(event_id);
            }
        }
        None
    }

    /// Dequeue up to `limit` events from the queue, in priority order.
    ///
    /// Returns a vector of event IDs ordered by priority (highest first).
    pub fn dequeue_batch(&mut self, limit: usize) -> Vec<EventId> {
        let mut result = Vec::with_capacity(limit.min(self.len()));
        while result.len() < limit {
            match self.dequeue() {
                Some(event_id) => result.push(event_id),
                None => break,
            }
        }
        result
    }

    /// Get the total number of queued events.
    pub fn len(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queues.iter().all(|q| q.is_empty())
    }

    /// Get the number of queued events at a specific priority level.
    pub fn len_by_priority(&self, priority: GossipPriority) -> usize {
        self.queues[priority as usize].len()
    }

    /// Check if the queue at a specific priority level is empty.
    pub fn is_empty_priority(&self, priority: GossipPriority) -> bool {
        self.queues[priority as usize].is_empty()
    }

    /// Get the total number of events ever enqueued.
    pub fn total_enqueued(&self) -> u64 {
        self.total_enqueued
    }

    /// Get the total number of events dropped due to capacity.
    pub fn total_dropped(&self) -> u64 {
        self.total_dropped
    }

    /// Get a reference to the queue configuration.
    pub fn config(&self) -> &PriorityQueueConfig {
        &self.config
    }

    /// Peek at the highest-priority event without removing it.
    pub fn peek(&self) -> Option<&EventId> {
        for priority in GossipPriority::all_descending() {
            let queue = &self.queues[*priority as usize];
            if let Some(event_id) = queue.front() {
                return Some(event_id);
            }
        }
        None
    }

    /// Clear all queues.
    pub fn clear(&mut self) {
        for queue in &mut self.queues {
            queue.clear();
        }
        self.seen.clear();
    }

    /// Check if a specific event ID is in the queue at any priority level.
    pub fn contains(&self, event_id: &EventId) -> bool {
        self.seen.contains(event_id)
    }

    /// Remove a specific event ID from the queue (if present).
    ///
    /// Returns `true` if the event was found and removed.
    pub fn remove(&mut self, event_id: &EventId) -> bool {
        for queue in &mut self.queues {
            if let Some(pos) = queue.iter().position(|id| id == event_id) {
                queue.remove(pos);
                self.seen.remove(event_id);
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn eid(id: u8) -> EventId {
        let mut eid = [0u8; 32];
        eid[0] = id;
        eid
    }

    #[test]
    fn test_priority_ordering() {
        assert!(GossipPriority::Critical > GossipPriority::High);
        assert!(GossipPriority::High > GossipPriority::Normal);
        assert!(GossipPriority::Normal > GossipPriority::Low);
    }

    #[test]
    fn test_priority_classify() {
        assert_eq!(GossipPriority::classify(true, false, false), GossipPriority::Critical);
        assert_eq!(GossipPriority::classify(false, true, false), GossipPriority::High);
        assert_eq!(GossipPriority::classify(false, false, true), GossipPriority::Low);
        assert_eq!(GossipPriority::classify(false, false, false), GossipPriority::Normal);
    }

    #[test]
    fn test_priority_display() {
        assert_eq!(format!("{}", GossipPriority::Critical), "CRITICAL");
        assert_eq!(format!("{}", GossipPriority::High), "HIGH");
        assert_eq!(format!("{}", GossipPriority::Normal), "NORMAL");
        assert_eq!(format!("{}", GossipPriority::Low), "LOW");
    }

    #[test]
    fn test_enqueue_dequeue_priority_order() {
        let mut queue = PriorityGossipQueue::with_defaults();

        // Enqueue in reverse priority order
        queue.enqueue(eid(1), GossipPriority::Low);
        queue.enqueue(eid(2), GossipPriority::Normal);
        queue.enqueue(eid(3), GossipPriority::High);
        queue.enqueue(eid(4), GossipPriority::Critical);

        // Dequeue should return in priority order
        assert_eq!(queue.dequeue(), Some(eid(4))); // Critical
        assert_eq!(queue.dequeue(), Some(eid(3))); // High
        assert_eq!(queue.dequeue(), Some(eid(2))); // Normal
        assert_eq!(queue.dequeue(), Some(eid(1))); // Low
        assert_eq!(queue.dequeue(), None);
    }

    #[test]
    fn test_fifo_within_priority() {
        let mut queue = PriorityGossipQueue::with_defaults();

        queue.enqueue(eid(1), GossipPriority::Normal);
        queue.enqueue(eid(2), GossipPriority::Normal);
        queue.enqueue(eid(3), GossipPriority::Normal);

        assert_eq!(queue.dequeue(), Some(eid(1)));
        assert_eq!(queue.dequeue(), Some(eid(2)));
        assert_eq!(queue.dequeue(), Some(eid(3)));
    }

    #[test]
    fn test_bounded_capacity() {
        let config = PriorityQueueConfig {
            max_critical: 2,
            max_high: 2,
            max_normal: 2,
            max_low: 2,
        };
        let mut queue = PriorityGossipQueue::new(config);

        // Fill the Normal queue beyond capacity
        queue.enqueue(eid(1), GossipPriority::Normal);
        queue.enqueue(eid(2), GossipPriority::Normal);
        queue.enqueue(eid(3), GossipPriority::Normal); // Should drop eid(1)

        assert_eq!(queue.len_by_priority(GossipPriority::Normal), 2);
        assert_eq!(queue.dequeue(), Some(eid(2))); // eid(1) was dropped
        assert_eq!(queue.dequeue(), Some(eid(3)));
    }

    #[test]
    fn test_empty_queue() {
        let mut queue = PriorityGossipQueue::with_defaults();
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
        assert_eq!(queue.dequeue(), None);
        assert_eq!(queue.peek(), None);
    }

    #[test]
    fn test_len_by_priority() {
        let mut queue = PriorityGossipQueue::with_defaults();

        queue.enqueue(eid(1), GossipPriority::Critical);
        queue.enqueue(eid(2), GossipPriority::Critical);
        queue.enqueue(eid(3), GossipPriority::Normal);

        assert_eq!(queue.len_by_priority(GossipPriority::Critical), 2);
        assert_eq!(queue.len_by_priority(GossipPriority::High), 0);
        assert_eq!(queue.len_by_priority(GossipPriority::Normal), 1);
        assert_eq!(queue.len_by_priority(GossipPriority::Low), 0);
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn test_dequeue_batch() {
        let mut queue = PriorityGossipQueue::with_defaults();

        queue.enqueue(eid(1), GossipPriority::Low);
        queue.enqueue(eid(2), GossipPriority::Critical);
        queue.enqueue(eid(3), GossipPriority::Normal);

        let batch = queue.dequeue_batch(2);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0], eid(2)); // Critical first
        assert_eq!(batch[1], eid(3)); // Normal next
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_peek() {
        let mut queue = PriorityGossipQueue::with_defaults();

        queue.enqueue(eid(1), GossipPriority::Normal);
        queue.enqueue(eid(2), GossipPriority::Critical);

        assert_eq!(queue.peek(), Some(&eid(2)));
        assert_eq!(queue.len(), 2); // peek doesn't remove
    }

    #[test]
    fn test_clear() {
        let mut queue = PriorityGossipQueue::with_defaults();

        queue.enqueue(eid(1), GossipPriority::Critical);
        queue.enqueue(eid(2), GossipPriority::Normal);
        queue.clear();

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn test_contains() {
        let mut queue = PriorityGossipQueue::with_defaults();

        queue.enqueue(eid(1), GossipPriority::Critical);
        assert!(queue.contains(&eid(1)));
        assert!(!queue.contains(&eid(2)));
    }

    #[test]
    fn test_remove() {
        let mut queue = PriorityGossipQueue::with_defaults();

        queue.enqueue(eid(1), GossipPriority::Critical);
        queue.enqueue(eid(2), GossipPriority::Critical);

        assert!(queue.remove(&eid(1)));
        assert!(!queue.contains(&eid(1)));
        assert!(queue.contains(&eid(2)));
        assert_eq!(queue.len(), 1);

        // Removing non-existent event should return false
        assert!(!queue.remove(&eid(99)));
    }

    #[test]
    fn test_statistics() {
        let config = PriorityQueueConfig {
            max_critical: 2,
            max_high: 100,
            max_normal: 100,
            max_low: 100,
        };
        let mut queue = PriorityGossipQueue::new(config);

        queue.enqueue(eid(1), GossipPriority::Critical);
        queue.enqueue(eid(2), GossipPriority::Critical);
        queue.enqueue(eid(3), GossipPriority::Critical); // drops eid(1)

        assert_eq!(queue.total_enqueued(), 3);
        assert_eq!(queue.total_dropped(), 1);
    }

    #[test]
    fn test_config_default() {
        let config = PriorityQueueConfig::default();
        assert_eq!(config.max_critical, 1000);
        assert_eq!(config.max_high, 5000);
        assert_eq!(config.max_normal, 10000);
        assert_eq!(config.max_low, 5000);
    }

    #[test]
    fn test_config_max_for_priority() {
        let config = PriorityQueueConfig::default();
        assert_eq!(config.max_for_priority(GossipPriority::Critical), 1000);
        assert_eq!(config.max_for_priority(GossipPriority::High), 5000);
        assert_eq!(config.max_for_priority(GossipPriority::Normal), 10000);
        assert_eq!(config.max_for_priority(GossipPriority::Low), 5000);
    }

    #[test]
    fn test_is_empty_priority() {
        let mut queue = PriorityGossipQueue::with_defaults();

        assert!(queue.is_empty_priority(GossipPriority::Critical));
        queue.enqueue(eid(1), GossipPriority::Critical);
        assert!(!queue.is_empty_priority(GossipPriority::Critical));
        assert!(queue.is_empty_priority(GossipPriority::Normal));
    }

    #[test]
    fn test_mixed_priority_dequeue() {
        let mut queue = PriorityGossipQueue::with_defaults();

        // Enqueue events in interleaved order
        queue.enqueue(eid(1), GossipPriority::Normal);
        queue.enqueue(eid(2), GossipPriority::High);
        queue.enqueue(eid(3), GossipPriority::Low);
        queue.enqueue(eid(4), GossipPriority::Critical);
        queue.enqueue(eid(5), GossipPriority::Normal);

        // Dequeue should be strictly priority-ordered
        assert_eq!(queue.dequeue(), Some(eid(4))); // Critical
        assert_eq!(queue.dequeue(), Some(eid(2))); // High
        assert_eq!(queue.dequeue(), Some(eid(1))); // Normal (FIFO)
        assert_eq!(queue.dequeue(), Some(eid(5))); // Normal (FIFO)
        assert_eq!(queue.dequeue(), Some(eid(3))); // Low
        assert_eq!(queue.dequeue(), None);
    }
}
