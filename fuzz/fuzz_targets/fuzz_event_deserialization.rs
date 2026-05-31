//! Fuzz target: Event deserialization
//!
//! Events arrive from untrusted network peers. This target ensures that
//! deserializing arbitrary bytes as an Event never panics, and that
//! validation handles malformed events gracefully.

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_primitives::Event;

fuzz_target!(|data: &[u8]| {
    // If deserialization succeeds, test that validation doesn't panic
    if let Ok(event) = postcard::from_bytes::<Event>(data) {
        // Validation should return Err, not panic
        let _ = event.validate();
    }
});
