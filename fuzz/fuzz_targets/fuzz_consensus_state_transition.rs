//! Fuzz target: Consensus state transition
//!
//! The consensus engine processes events from untrusted sources. Invalid
//! state transitions must be rejected, not cause panics. This target
//! ensures that feeding arbitrary deserialized events to the consensus
//! engine never panics.
//!
//! Note: process_event requires a &CausalGraph, so we create a minimal
//! empty graph for each fuzz input. Events without valid parents in the
//! graph will be rejected, but the rejection must be an error, not a panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_substrate::{CausalGraph, ConsensusConfig, ConsensusEngine, Event, SlashingEngine};

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
        let graph = CausalGraph::new();

        for event in events {
            // process_event must never panic, even on invalid inputs
            let _ = engine.process_event(&event, &graph);
        }
    }
});
