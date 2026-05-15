//! Fuzz target: Vector clock merge
//!
//! Vector clocks are CRDTs that merge state from untrusted peers. The
//! merge operation must be total and never panic. This target ensures
//! that deserializing and merging arbitrary vector clocks never panics,
//! and that CRDT properties (idempotency, commutativity) hold.

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_substrate::VectorClock;

fuzz_target!(|data: &[u8]| {
    // Try to deserialize two vector clocks and merge them
    if let Ok((a, b)) = bincode::deserialize::<(VectorClock, VectorClock)>(data) {
        // Merge must never panic
        let _merged = a.merged(&b);

        // Verify CRDT properties hold after merge
        // Idempotency: merge(a, a) == a
        let idempotent = a.merged(&a);
        assert_eq!(idempotent, a, "VectorClock merge is not idempotent!");

        // Commutativity: merge(a, b) == merge(b, a)
        let ab = a.merged(&b);
        let ba = b.merged(&a);
        assert_eq!(ab, ba, "VectorClock merge is not commutative!");
    }

    // Also test the custom binary format
    if let Ok(a) = VectorClock::from_bytes(data) {
        // from_bytes must never panic on valid return
        let _merged = a.merged(&a);
    }
});
