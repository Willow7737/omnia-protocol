//! Thread pool for parallel event validation
//!
//! Uses a bounded thread pool to validate events in parallel across
//! shards. The pool is designed for CPU-bound work (signature verification,
//! hash computation) with minimal coordination overhead.
//!
//! # Design
//!
//! - Workers receive tasks via `std::sync::mpsc` channels (one per worker)
//! - Round-robin distribution across workers
//! - Results are collected via a shared `RwLock<Vec<ValidationResult>>`
//! - Graceful shutdown via `Shutdown` task variant
//!
//! # Usage
//!
//! ```ignore
//! use omnia_consensus::thread_pool::{ValidationPool, ValidationTask};
//! use std::sync::Arc;
//! use omnia_consensus::ShardedConsensusState;
//!
//! let state = Arc::new(ShardedConsensusState::new());
//! let pool = ValidationPool::new(4, Arc::clone(&state));
//!
//! // Submit events for validation
//! pool.submit(ValidationTask::ValidateEvent(event))?;
//!
//! // Collect results
//! let results = pool.drain_results();
//!
//! // Graceful shutdown
//! pool.shutdown();
//! ```

use crate::sharded_state::ShardedConsensusState;
use omnia_primitives::{Event, EventId};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread;

/// Task for the thread pool.
#[derive(Debug)]
pub enum ValidationTask {
    /// Validate and insert a single event into the sharded state.
    ValidateEvent(Box<Event>),
    /// Shutdown the worker.
    Shutdown,
}

/// Result of event validation.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// The event ID that was validated.
    pub event_id: EventId,
    /// Whether validation succeeded.
    pub valid: bool,
    /// Error message if validation failed.
    pub error: Option<String>,
}

/// Thread pool for parallel event validation.
///
/// Each worker owns a receiver for its dedicated channel. Tasks are
/// distributed round-robin across workers. Results are written to a
/// shared vector protected by an `RwLock`.
pub struct ValidationPool {
    /// Task senders, one per worker.
    senders: Vec<Sender<ValidationTask>>,
    /// Worker thread handles.
    workers: Vec<thread::JoinHandle<()>>,
    /// Shared result collector.
    results: Arc<RwLock<Vec<ValidationResult>>>,
    /// Number of workers.
    num_workers: usize,
    /// Next worker index for round-robin distribution.
    next_worker: std::sync::atomic::AtomicUsize,
}

impl ValidationPool {
    /// Create a new validation pool with the given number of workers.
    ///
    /// Each worker receives a reference to the shared [`ShardedConsensusState`]
    /// and processes validation tasks independently.
    ///
    /// # Arguments
    ///
    /// * `num_workers` — Number of worker threads. If 0, defaults to 1.
    /// * `state` — Shared sharded consensus state for event insertion.
    pub fn new(num_workers: usize, state: Arc<ShardedConsensusState>) -> Self {
        let num_workers = num_workers.max(1);
        let results = Arc::new(RwLock::new(Vec::new()));
        let mut senders = Vec::with_capacity(num_workers);
        let mut workers = Vec::with_capacity(num_workers);

        for worker_id in 0..num_workers {
            let (tx, rx) = channel::<ValidationTask>();
            senders.push(tx);

            let state = Arc::clone(&state);
            let results = Arc::clone(&results);

            let handle = thread::Builder::new()
                .name(format!("omnia-validation-worker-{worker_id}"))
                .spawn(move || {
                    Self::worker_loop(worker_id, rx, &state, &results);
                })
                .expect("failed to spawn validation worker thread");

            workers.push(handle);
        }

        Self {
            senders,
            workers,
            results,
            num_workers,
            next_worker: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Worker main loop.
    ///
    /// Receives tasks from the channel and processes them. Exits
    /// gracefully on `Shutdown`.
    fn worker_loop(
        worker_id: usize,
        rx: Receiver<ValidationTask>,
        state: &ShardedConsensusState,
        results: &RwLock<Vec<ValidationResult>>,
    ) {
        tracing::debug!(worker_id, "validation worker started");

        loop {
            let task = match rx.recv() {
                Ok(task) => task,
                Err(_) => {
                    // Channel disconnected, exit
                    tracing::debug!(worker_id, "validation worker channel disconnected, exiting");
                    break;
                }
            };

            match task {
                ValidationTask::Shutdown => {
                    tracing::debug!(worker_id, "validation worker received shutdown");
                    break;
                }
                ValidationTask::ValidateEvent(event) => {
                    let result = Self::validate_and_insert(&event, state);
                    let mut results_guard = results.write().unwrap_or_else(|e| e.into_inner());
                    results_guard.push(result);
                }
            }
        }

        tracing::debug!(worker_id, "validation worker exiting");
    }

    /// Validate a single event and insert it into the sharded state.
    ///
    /// Performs:
    /// 1. Hash integrity check
    /// 2. Event insertion into the sharded state (if not already present)
    ///
    /// Returns a [`ValidationResult`] indicating success or failure.
    fn validate_and_insert(event: &Event, state: &ShardedConsensusState) -> ValidationResult {
        let event_id = event.id;

        // Hash integrity check
        if !event.verify_hash().unwrap_or(false) {
            return ValidationResult {
                event_id,
                valid: false,
                error: Some("hash integrity check failed".to_string()),
            };
        }

        // Atomically insert into sharded state — detects duplicates correctly
        // even under concurrent access because each shard is protected by its
        // own RwLock and the check+insert happens while holding the write lock.
        let inserted = state.insert_event_state_if_absent(event_id, crate::consensus::ConsensusState::Pending);

        if !inserted {
            return ValidationResult {
                event_id,
                valid: true,
                error: Some("already processed".to_string()),
            };
        }

        ValidationResult {
            event_id,
            valid: true,
            error: None,
        }
    }

    /// Submit a task to the pool using round-robin distribution.
    ///
    /// Tasks are distributed across workers in round-robin fashion.
    /// If the target worker's channel is disconnected (e.g., the worker
    /// panicked), returns `Err(task)` so the caller can handle the failure.
    pub fn submit(&self, task: ValidationTask) -> Result<(), ValidationTask> {
        let worker_idx = self.next_worker.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % self.num_workers;
        self.senders[worker_idx].send(task).map_err(|e| e.0)
    }

    /// Drain all collected results.
    ///
    /// Returns all validation results that have been accumulated by
    /// workers since the last call to `drain_results`. The internal
    /// buffer is cleared.
    pub fn drain_results(&self) -> Vec<ValidationResult> {
        let mut results_guard = self.results.write().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *results_guard)
    }

    /// Get the number of pending results (without draining).
    pub fn pending_results(&self) -> usize {
        let results_guard = self.results.read().unwrap_or_else(|e| e.into_inner());
        results_guard.len()
    }

    /// Get the number of workers in the pool.
    pub fn num_workers(&self) -> usize {
        self.num_workers
    }

    /// Shut down the pool gracefully.
    ///
    /// Sends a `Shutdown` task to each worker and waits for all
    /// worker threads to exit. After shutdown, no more tasks can
    /// be submitted.
    pub fn shutdown(self) {
        for (i, sender) in self.senders.into_iter().enumerate() {
            if let Err(e) = sender.send(ValidationTask::Shutdown) {
                tracing::warn!(worker_id = i, error = %e, "failed to send shutdown to worker");
            }
        }

        for (i, handle) in self.workers.into_iter().enumerate() {
            match handle.join() {
                Ok(()) => {
                    tracing::debug!(worker_id = i, "worker shut down cleanly");
                }
                Err(_) => {
                    tracing::warn!(worker_id = i, "worker panicked during shutdown");
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_primitives::VectorClock;
    use rand::rngs::OsRng;
    use rand::RngCore;

    fn make_test_event(seq: u64) -> Event {
        let mut creator = [0u8; 32];
        OsRng.fill_bytes(&mut creator);

        let vc = VectorClock::with_node(creator, seq + 1);
        let mut event = Event::new(creator, seq, vc, None, None, vec![1, 2, 3]).expect("valid event");

        // Use a real keypair for proper signing
        let keypair = ed25519_dalek::SigningKey::generate(&mut OsRng);
        event.sign_with_keypair(&keypair).expect("signing");

        event
    }

    #[test]
    fn test_pool_basic_validation() {
        let state = Arc::new(ShardedConsensusState::new());
        let pool = ValidationPool::new(4, Arc::clone(&state));

        let event = make_test_event(0);
        let event_id = event.id;

        pool.submit(ValidationTask::ValidateEvent(Box::new(event))).expect("submit should succeed");
        thread::sleep(std::time::Duration::from_millis(100));

        let results = pool.drain_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_id, event_id);
        assert!(results[0].valid);

        // Verify the event was inserted into the sharded state
        assert!(state.contains_event(&event_id));

        pool.shutdown();
    }

    #[test]
    fn test_pool_multiple_events() {
        let state = Arc::new(ShardedConsensusState::new());
        let pool = ValidationPool::new(4, Arc::clone(&state));

        let num_events: usize = 20;
        let mut event_ids = Vec::new();

        for seq in 0..num_events {
            let event = make_test_event(seq as u64);
            event_ids.push(event.id);
            pool.submit(ValidationTask::ValidateEvent(Box::new(event))).expect("submit should succeed");
        }

        // Give workers time to process
        thread::sleep(std::time::Duration::from_millis(500));

        let results = pool.drain_results();
        assert_eq!(results.len(), num_events);

        // All should be valid
        for result in &results {
            assert!(result.valid, "Event {:?} should be valid", &result.event_id[..4]);
        }

        // All events should be in the sharded state
        for event_id in &event_ids {
            assert!(state.contains_event(event_id));
        }

        pool.shutdown();
    }

    #[test]
    fn test_pool_duplicate_event() {
        let state = Arc::new(ShardedConsensusState::new());
        let pool = ValidationPool::new(2, Arc::clone(&state));

        let event = make_test_event(0);

        // Submit the same event twice
        let _ = pool.submit(ValidationTask::ValidateEvent(Box::new(event.clone())));
        let _ = pool.submit(ValidationTask::ValidateEvent(Box::new(event.clone())));

        // Give workers time to process
        thread::sleep(std::time::Duration::from_millis(200));

        let results = pool.drain_results();
        // Both results should be present but the second should note "already processed"
        assert_eq!(results.len(), 2);
        assert!(results[0].valid);
        assert!(results[1].valid);
        // One of them should have the "already processed" note
        let already_processed_count = results
            .iter()
            .filter(|r| {
                r.error
                    .as_ref()
                    .map(|e| e.contains("already processed"))
                    .unwrap_or(false)
            })
            .count();
        assert!(
            already_processed_count >= 1,
            "At least one result should note 'already processed'"
        );

        pool.shutdown();
    }

    #[test]
    fn test_pool_shutdown() {
        let state = Arc::new(ShardedConsensusState::new());
        let pool = ValidationPool::new(2, Arc::clone(&state));
        assert_eq!(pool.num_workers(), 2);

        // Shutdown should complete without hanging
        pool.shutdown();
    }

    #[test]
    fn test_pool_default_workers() {
        let state = Arc::new(ShardedConsensusState::new());
        let pool = ValidationPool::new(0, Arc::clone(&state));
        assert_eq!(pool.num_workers(), 1); // 0 defaults to 1
        pool.shutdown();
    }
}
