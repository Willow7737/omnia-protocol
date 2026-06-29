//! Fuzz target: Snapshot deserialization
//!
//! Snapshots are loaded from disk. A corrupted snapshot file must not
//! cause a panic. This target ensures that deserializing arbitrary bytes
//! as a StateSnapshot never panics, and that integrity verification
//! handles tampered data gracefully.

#![no_main]
// F-23 fix: removed stale #![allow(deprecated)] — StateSnapshot has no
// #[deprecated] attribute, and omnia-substrate is not a deprecated crate.

use libfuzzer_sys::fuzz_target;
use omnia_substrate::StateSnapshot;

fuzz_target!(|data: &[u8]| {
    // Test that snapshot deserialization never panics on arbitrary bytes
    if let Ok(snapshot) = StateSnapshot::from_bytes(data) {
        // verify() should return false for tampered snapshots, not panic
        let _ = snapshot.verify();
    }
});
