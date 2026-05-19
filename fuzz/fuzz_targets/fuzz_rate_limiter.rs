//! Fuzz target: Rate limiter
//!
//! The rate limiter tracks per-peer state. It must handle arbitrary
//! sequences of `allow()` and `reset()` calls without panicking.

#![no_main]

use libfuzzer_sys::fuzz_target;
use omnia_consensus::RateLimiter;

fuzz_target!(|data: &[u8]| {
    let mut limiter = RateLimiter::new(200, 100);

    // Interpret each byte as an operation
    for &byte in data {
        let peer_id = [byte; 32];
        match byte % 3 {
            0 => {
                let _ = limiter.allow(&peer_id);
            }
            1 => {
                limiter.reset(&peer_id);
            }
            _ => {
                let _ = limiter.allow(&peer_id);
            }
        }
    }
});
