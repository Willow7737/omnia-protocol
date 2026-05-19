//! Fuzz target: ZK proof bundle deserialization
//!
//! ZK proofs arrive from untrusted provers. Malformed proofs must be
//! rejected without panicking. This target tests deserialization of
//! ProofBundle (which uses postcard) and structural validation.
//!
//! Note: Full pairing-check verification is intentionally omitted —
//! it is too slow for fuzzing. We only test deserialization + integrity.

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_adapters::ProofBundle;

fuzz_target!(|data: &[u8]| {
    // Test that ProofBundle deserialization never panics on arbitrary input
    if let Ok(bundle) = ProofBundle::from_bytes(data) {
        // If deserialization succeeds, test that integrity check doesn't panic
        let _ = bundle.verify_integrity();
    }
});
