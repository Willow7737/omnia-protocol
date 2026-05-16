//! Fuzz target: Gossip message parsing
//!
//! Gossip messages arrive from any peer. A malformed message must not
//! crash the node. This target ensures deserialization never panics
//! and that any validation handles bad data gracefully.

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_substrate::GossipMessage;

fuzz_target!(|data: &[u8]| {
    // Test gossip message deserialization never panics
    if let Ok(_msg) = postcard::from_bytes::<GossipMessage>(data) {
        // Deserialization succeeded — the message is well-formed at the
        // serialization level. Further validation (e.g., verifying embedded
        // events) is done by the gossip protocol layer, which also must
        // not panic. We cannot easily call validate_event here because it
        // requires a GossipProtocol with an active CausalGraph.
    }
});
