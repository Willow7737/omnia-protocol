#![no_main]
use libfuzzer_sys::fuzz_target;
use omnia_shards::ShardRouter;
use omnia_substrate::{Event, VectorClock};

fuzz_target!(|data: &[u8]| {
    if data.len() < 64 {
        return;
    }
    let router = ShardRouter::new();
    let mut creator = [0u8; 32];
    creator.copy_from_slice(&data[0..32]);
    let payload = data[64..].to_vec();
    let clock = VectorClock::new();
    let event = Event::genesis(creator, payload);
    // route_event deserializes the payload as ShardPayload,
    // so fuzzing with arbitrary bytes will exercise error paths
    let mut router = router;
    let _ = router.route_event(&event);
});
