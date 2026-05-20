//! Fuzz target: Primitives Event serialization roundtrip
//!
//! Events arrive from untrusted network peers and must be deserialized
//! safely. This target ensures that deserializing arbitrary bytes as an
//! Event using the versioned wire format never panics, and that a
//! successful deserialization → re-serialization roundtrip is idempotent.

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_primitives::{deserialize_with_version, serialize_with_version, Event};

fuzz_target!(|data: &[u8]| {
    // Test versioned wire-format deserialization never panics
    if let Ok(event) = deserialize_with_version::<Event>(data) {
        // If deserialization succeeded, re-serialization should also work
        if let Ok(re_serialized) = serialize_with_version(&event) {
            // Roundtrip: re-deserializing should produce the same event
            if let Ok(event2) = deserialize_with_version::<Event>(&re_serialized) {
                assert_eq!(event, event2, "Event roundtrip mismatch!");
            }
        }
    }
});
