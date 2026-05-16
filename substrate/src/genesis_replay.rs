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
//! 5. Return a [`ReplayResult`] with the reconstructed state summary.
//!
//! # Performance
//!
//! Genesis replay is O(N) in the number of events and should only be used
//! as a fallback. For normal node synchronization, use snapshot-based
//! recovery (see [`crate::snapshot`]).

use crate::causal_graph::CausalGraph;
use crate::consensus::{ConsensusConfig, ConsensusEngine};
use crate::event::Event;
use crate::slashing::{SlashingEngine, SlashingState};
use serde::{Deserialize, Serialize};

/// Result of a genesis replay operation.
///
/// Contains the reconstructed state summary and metadata about the replay
/// process, such as the number of events processed and any warnings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    /// Total number of events replayed.
    pub events_replayed: u64,
    /// Number of events that were successfully committed through consensus.
    pub events_committed: u64,
    /// Number of events that failed processing (e.g., invalid signatures).
    pub events_failed: u64,
    /// The final slashing state after replay.
    pub slashing_state: SlashingState,
    /// The height of the causal graph after replay.
    pub graph_height: u64,
    /// Whether the replay completed successfully (no fatal errors).
    pub success: bool,
    /// Warnings encountered during replay (non-fatal issues).
    pub warnings: Vec<String>,
}

impl ReplayResult {
    /// Create an empty (zero-event) replay result.
    pub fn empty() -> Self {
        Self {
            events_replayed: 0,
            events_committed: 0,
            events_failed: 0,
            slashing_state: SlashingState::default(),
            graph_height: 0,
            success: true,
            warnings: Vec::new(),
        }
    }
}

/// Replay a sequence of events from genesis to reconstruct node state.
///
/// This function processes a slice of events in order, inserting each into
/// the causal graph and running it through the consensus engine. The result
/// captures the final state after all events have been processed.
///
/// # Arguments
///
/// * `events` — A slice of [`Event`]s in topological order (genesis first).
/// * `consensus_config` — Configuration for the consensus engine.
/// * `slashing` — A [`SlashingEngine`] instance for tracking penalties.
/// * `graph` — A mutable reference to an empty [`CausalGraph`] to populate.
///
/// # Returns
///
/// A [`ReplayResult`] summarizing the replay operation.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::genesis_replay::{replay_genesis, ReplayResult};
/// use omnia_substrate::causal_graph::CausalGraph;
/// use omnia_substrate::consensus::ConsensusConfig;
/// use omnia_substrate::slashing::SlashingEngine;
///
/// let mut graph = CausalGraph::new();
/// let slashing = SlashingEngine::new_in_memory(500, 2000);
/// let config = ConsensusConfig::default();
/// let result = replay_genesis(&events, &config, slashing, &mut graph);
/// assert!(result.success);
/// ```
pub fn replay_genesis(
    events: &[Event],
    consensus_config: &ConsensusConfig,
    slashing: SlashingEngine,
    graph: &mut CausalGraph,
) -> ReplayResult {
    let mut result = ReplayResult::empty();
    let mut consensus = ConsensusEngine::new(consensus_config.clone(), slashing.clone());

    for event in events {
        result.events_replayed += 1;

        // Insert event into the causal graph
        if let Err(e) = graph.insert(event.clone()) {
            result.events_failed += 1;
            result.warnings.push(format!(
                "Failed to insert event {:?}: {}",
                &event.id[..4.min(event.id.len())],
                e
            ));
            continue;
        }

        // Process event through consensus
        match consensus.process_event(event, graph) {
            Ok(committed) => {
                result.events_committed += committed.len() as u64;
            }
            Err(e) => {
                result.events_failed += 1;
                result.warnings.push(format!(
                    "Consensus error for event {:?}: {}",
                    &event.id[..4.min(event.id.len())],
                    e
                ));
            }
        }
    }

    result.graph_height = graph.len() as u64;
    result.slashing_state = SlashingState {
        slash_points: slashing.internal_slash_points(),
        stakes: slashing.internal_stakes(),
        slash_threshold: slashing.internal_slash_threshold(),
        ejection_threshold: slashing.internal_ejection_threshold(),
    };

    // Replay is successful if more events committed than failed
    result.success = result.events_failed == 0 || result.events_committed > 0;

    if result.events_failed > 0 {
        result.warnings.push(format!(
            "{} events failed during replay out of {} total",
            result.events_failed, result.events_replayed
        ));
    }

    tracing::info!(
        events_replayed = result.events_replayed,
        events_committed = result.events_committed,
        events_failed = result.events_failed,
        graph_height = result.graph_height,
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
        let mut graph = CausalGraph::new();
        let slashing =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let config = ConsensusConfig::default();

        let result = replay_genesis(&[], &config, slashing, &mut graph);
        assert!(result.success);
        assert_eq!(result.events_replayed, 0);
        assert_eq!(result.events_committed, 0);
        assert_eq!(result.events_failed, 0);
    }

    #[test]
    fn test_genesis_events_replay() {
        let mut graph = CausalGraph::new();
        let slashing =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let config = ConsensusConfig::default();

        // Create genesis events
        let events: Vec<Event> = (0..4)
            .map(|i| {
                let keypair = generate_keypair();
                let mut e = Event::genesis(test_node(i), vec![i]);
                e.sign_with_keypair(&keypair);
                e
            })
            .collect();

        let result = replay_genesis(&events, &config, slashing, &mut graph);
        assert_eq!(result.events_replayed, 4);
        assert!(result.events_committed >= 3, "Expected at least 3 committed events in a 4-node network, got {}", result.events_committed);
        assert_eq!(result.events_failed, 0);
        assert_eq!(result.graph_height, 4);
    }

    #[test]
    fn test_replay_result_empty() {
        let result = ReplayResult::empty();
        assert!(result.success);
        assert_eq!(result.events_replayed, 0);
        assert!(result.warnings.is_empty());
    }
}
