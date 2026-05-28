#![no_main]
use libfuzzer_sys::fuzz_target;
use omnia_consensus::CausalGraph;
use omnia_primitives::{Event, VectorClock};

fuzz_target!(|data: &[u8]| {
    if data.len() < 64 {
        return;
    }
    let mut creator = [0u8; 32];
    creator.copy_from_slice(&data[0..32]);
    let payload = data[64..].to_vec();
    let clock = VectorClock::new();
    // Genesis event (no parents required)
    let event = Event::genesis(creator, payload).unwrap();
    let mut graph = CausalGraph::new();
    let _ = graph.insert(event);
    // Try inserting a second event that links to the first
    if let Some(first_id) = graph.event_ids().first().copied() {
        let event2 = Event::new(creator, 1, clock, Some(first_id), None, data[32..64].to_vec()).unwrap();
        let _ = graph.insert(event2);
    }
});
