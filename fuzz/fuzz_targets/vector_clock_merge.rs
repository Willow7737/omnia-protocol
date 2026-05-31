//! Fuzz target: Vector clock merge (raw binary format)
//!
//! This target tests vector clock merge using a custom binary format
//! where 33-byte chunks are interpreted as (NodeId, counter) pairs.
//! It is intentionally kept separate from `fuzz_vector_clock_merge.rs`,
//! which uses postcard deserialization and also verifies CRDT properties
//! (idempotency, commutativity). The two targets exercise different
//! code paths and input grammars, so both are retained.

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_primitives::VectorClock;

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
    clock1.merge(&clock2);
});
