#![no_main]
use libfuzzer_sys::fuzz_target;
use omnia_substrate::VectorClock;

fuzz_target!(|data: &[u8]| {
    let mut clock1 = VectorClock::new();
    let mut clock2 = VectorClock::new();
    for chunk in data.chunks(33) {
        if chunk.len() == 33 {
            let mut node = [0u8; 32];
            node.copy_from_slice(&chunk[0..32]);
            let _ = clock1.increment(node);
            if chunk[32] % 2 == 0 {
                let _ = clock2.increment(node);
            }
        }
    }
    let _ = clock1.merge(&clock2);
});
