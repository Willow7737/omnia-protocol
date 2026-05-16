//! Genesis Replay — Disaster Recovery via State Reconstruction.
//!
//! This module provides the ability to replay the entire event history from
//! genesis to reconstruct node state after a catastrophic failure (e.g.,
//! corrupted database, hardware failure, or software bug). It is the last
//! line of defense when snapshot-based recovery is unavailable.
//!
//! # When to Use
//!
//! - A node has lost its database and has no recent snapshot.
//! - A snapshot integrity check fails and no valid backup exists.
//! - A new node joins the network and must sync from scratch.
//! - Forensic analysis of the full event history is required.
//!
//! # How It Works
//!
//! 1. Fetch the genesis event and all subsequent events from peers.
//! 2. Replay each event through the causal graph in topological order.
//! 3. Re-run consensus for each event to rebuild the consensus state.
//! 4. Re-apply slashing state from the event stream.
//! 5. Verify the reconstructed state root against the expected root.
//! 6. Return a [`ReplayResult`] with the reconstructed state summary.
//!
//! # Performance
//!
//! Genesis replay is O(N) in the number of events and should only be used
//! as a fallback. For normal node synchronization, use snapshot-based
//! recovery (see [`crate::snapshot`]).

use crate::causal_graph::CausalGraph;
use crate::consensus::ConsensusConfig;
use crate::consensus::ConsensusEngine;
use crate::event::Event;
use crate::slashing::{SlashingEngine, SlashingState};
use serde::{Deserialize, Serialize};

/// Configuration for a genesis replay operation.
///
/// Controls the consensus parameters and slashing thresholds used during
/// the replay. The replay constructs its own internal [`CausalGraph`] and
/// [`ConsensusEngine`] from these parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    /// Consensus configuration for the replay engine.
    pub consensus_config: ConsensusConfig,
    /// Slash threshold for the internal slashing engine.
    pub slash_threshold: u64,
    /// Ejection threshold for the internal slashing engine.
    pub ejection_threshold: u64,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            consensus_config: ConsensusConfig::default(),
            slash_threshold: crate::slashing::DEFAULT_SLASH_THRESHOLD,
            ejection_threshold: crate::slashing::DEFAULT_EJECTION_THRESHOLD,
        }
    }
}

/// Result of a genesis replay operation.
///
/// Contains the reconstructed state summary and metadata about the replay
/// process, such as the number of events processed and whether the
/// reconstructed state root matches the expected root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    /// Total number of events processed during replay.
    pub events_processed: u64,
    /// Number of events that were successfully finalized through consensus.
    pub events_finalized: u64,
    /// Number of events that were rejected (e.g., invalid signatures, consensus failure).
    pub events_rejected: u64,
    /// The size of the causal graph after replay (number of events).
    pub final_graph_size: u64,
    /// Whether the reconstructed state root matches the expected root.
    ///
    /// `None` if no expected root was provided for verification.
    /// `Some(true)` if the root matches.
    /// `Some(false)` if the root does not match (indicates data corruption).
    pub root_matches: Option<bool>,
}

impl ReplayResult {
    /// Create an empty (zero-event) replay result.
    pub fn empty() -> Self {
        Self {
            events_processed: 0,
            events_finalized: 0,
            events_rejected: 0,
            final_graph_size: 0,
            root_matches: None,
        }
    }
}

/// Replay a sequence of events from genesis to reconstruct node state.
///
/// This function processes a slice of events in order, inserting each into
/// a new causal graph and running it through the consensus engine. If an
/// `expected_root` is provided, the reconstructed state root is compared
/// against it after the replay completes.
///
/// # Arguments
///
/// * `events` — A slice of [`Event`]s in topological order (genesis first).
/// * `expected_root` — An optional expected state root hash for verification.
///   If `Some`, the replay will compare the computed root against this value.
/// * `config` — A [`ReplayConfig`] controlling consensus and slashing parameters.
///
/// # Returns
///
/// A [`ReplayResult`] summarizing the replay operation.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::genesis_replay::{replay_genesis, ReplayConfig, ReplayResult};
///
/// let config = ReplayConfig::default();
/// let result = replay_genesis(&events, Some(expected_root), &config);
/// assert!(result.root_matches.unwrap_or(false));
/// ```
pub fn replay_genesis(
    events: &[Event],
    expected_root: Option<&[u8; 32]>,
    config: &ReplayConfig,
) -> ReplayResult {
    let mut result = ReplayResult::empty();
    let mut graph = CausalGraph::new();
    let slashing = SlashingEngine::new_in_memory(config.slash_threshold, config.ejection_threshold);
    let mut consensus = ConsensusEngine::new(config.consensus_config.clone(), slashing.clone());

    for event in events {
        result.events_processed += 1;

        // Insert event into the causal graph
        if let Err(e) = graph.insert(event.clone()) {
            result.events_rejected += 1;
            tracing::warn!(
                "Failed to insert event {:?}: {}",
                &event.id[..4.min(event.id.len())],
                e
            );
            continue;
        }

        // Process event through consensus
        match consensus.process_event(event, &graph) {
            Ok(committed) => {
                result.events_finalized += committed.len() as u64;
            }
            Err(e) => {
                result.events_rejected += 1;
                tracing::warn!(
                    "Consensus error for event {:?}: {}",
                    &event.id[..4.min(event.id.len())],
                    e
                );
            }
        }
    }

    result.final_graph_size = graph.len() as u64;

    // Verify the state root if an expected root was provided
    if let Some(expected) = expected_root {
        let computed = graph.state_root();
        result.root_matches = Some(computed == *expected);
    }

    tracing::info!(
        events_processed = result.events_processed,
        events_finalized = result.events_finalized,
        events_rejected = result.events_rejected,
        final_graph_size = result.final_graph_size,
        root_matches = ?result.root_matches,
        "Genesis replay completed"
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::ConsensusConfig;
    use crate::crypto::generate_keypair;
    use crate::event::Event;
    use crate::slashing::{DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD};

    fn test_node(id: u8) -> [u8; 32] {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    #[test]
    fn test_empty_replay() {
        let config = ReplayConfig::default();
        let result = replay_genesis(&[], None, &config);

        assert_eq!(result.events_processed, 0);
        assert_eq!(result.events_finalized, 0);
        assert_eq!(result.events_rejected, 0);
        assert_eq!(result.final_graph_size, 0);
        assert!(result.root_matches.is_none());
    }

    #[test]
    fn test_empty_replay_with_expected_root() {
        let config = ReplayConfig::default();
        let expected = [0u8; 32]; // Empty graph should have this root
        let result = replay_genesis(&[], Some(&expected), &config);

        assert_eq!(result.events_processed, 0);
        assert!(result.root_matches.is_some());
    }

    #[test]
    fn test_genesis_events_replay() {
        let config = ReplayConfig {
            consensus_config: ConsensusConfig::default(),
            slash_threshold: DEFAULT_SLASH_THRESHOLD,
            ejection_threshold: DEFAULT_EJECTION_THRESHOLD,
        };

        // Create genesis events
        let events: Vec<Event> = (0..4)
            .map(|i| {
                let keypair = generate_keypair();
                let mut e = Event::genesis(test_node(i), vec![i]);
                e.sign_with_keypair(&keypair);
                e
            })
            .collect();

        let result = replay_genesis(&events, None, &config);
        assert_eq!(result.events_processed, 4);
        assert!(
            result.events_finalized >= 3,
            "Expected at least 3 finalized events in a 4-node network, got {}",
            result.events_finalized
        );
        assert_eq!(result.events_rejected, 0);
        assert_eq!(result.final_graph_size, 4);
    }

    #[test]
    fn test_rejection_tracking() {
        let config = ReplayConfig::default();

        // Create an event that will fail graph insertion (duplicate)
        let keypair = generate_keypair();
        let mut event1 = Event::genesis(test_node(1), vec![1]);
        event1.sign_with_keypair(&keypair);
        let event2 = event1.clone(); // Duplicate — should be rejected

        let result = replay_genesis(&[event1, event2], None, &config);
        assert_eq!(result.events_processed, 2);
        assert!(
            result.events_rejected >= 1,
            "Expected at least 1 rejected event for duplicate, got {}",
            result.events_rejected
        );
    }

    #[test]
    fn test_replay_result_empty() {
        let result = ReplayResult::empty();
        assert_eq!(result.events_processed, 0);
        assert_eq!(result.events_finalized, 0);
        assert_eq!(result.events_rejected, 0);
        assert_eq!(result.final_graph_size, 0);
        assert!(result.root_matches.is_none());
    }

    #[test]
    fn test_replay_config_default() {
        let config = ReplayConfig::default();
        assert_eq!(config.slash_threshold, DEFAULT_SLASH_THRESHOLD);
        assert_eq!(config.ejection_threshold, DEFAULT_EJECTION_THRESHOLD);
    }
}
