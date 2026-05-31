//! Fuzz target: Consensus state transition
//!
//! The consensus engine processes events from untrusted sources. Invalid
//! state transitions must be rejected, not cause panics. This target
//! ensures that feeding arbitrary deserialized events to the consensus
//! engine never panics.
//!
//! Events are first inserted into a mutable CausalGraph so that
//! process_event can perform ancestry queries. Events that fail graph
//! insertion are still fed to consensus (which will reject them).

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_consensus::{CausalGraph, ConsensusConfig, ConsensusEngine, SlashingEngine};
use omnia_primitives::Event;

fuzz_target!(|data: &[u8]| {
    // Deserialize a sequence of events and feed them to consensus
    if let Ok(events) = postcard::from_bytes::<Vec<Event>>(data) {
        let slashing = SlashingEngine::new_in_memory(10, 20);
        let mut seed = [0u8; 32];
        seed[0] = 1; // Non-zero to avoid debug-build panic
        let config = ConsensusConfig {
            round_seed: seed,
            ..ConsensusConfig::default()
        };
        let mut engine = ConsensusEngine::new(config, slashing);
        let mut graph = CausalGraph::new();

        for event in events {
            // Try inserting the event into the graph first so that
            // process_event can perform ancestry queries. If graph
            // insertion fails, we still feed the event to consensus
            // (which must handle missing-parent errors gracefully).
            let event_id = event.id;
            let _ = graph.insert(event.clone());
            let graph_event = graph.get(&event_id).unwrap_or(&event);
            // process_event must never panic, even on invalid inputs
            let _ = engine.process_event(graph_event, &graph);
        }
    }
});
