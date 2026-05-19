#![no_main]
use libfuzzer_sys::fuzz_target;
use omnia_primitives::{Event, VectorClock};

fuzz_target!(|data: &[u8]| {
    if data.len() < 64 {
        return;
    }
    let mut creator = [0u8; 32];
    creator.copy_from_slice(&data[0..32]);
    let payload = data[64..].to_vec();
    let clock = VectorClock::new();
    // Genesis event
    let event = Event::genesis(creator, payload);
    let _ = event.validate();
    // Event with parents
    let mut self_parent = [0u8; 32];
    self_parent.copy_from_slice(&data[32..64]);
    let event2 = Event::new(
        creator,
        1,
        clock,
        Some(self_parent),
        None,
        data[0..32].to_vec(),
    );
    let _ = event2.validate();
});
