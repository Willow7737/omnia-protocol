//! Token-bucket rate limiter for per-peer event rate limiting.
//!
//! Each peer gets a configurable number of events per second.
//! Events exceeding the rate are silently dropped with a warning log.
//! This prevents gossip flooding: a malicious peer cannot consume
//! disproportionate bandwidth or CPU.

use std::collections::HashMap;
use std::time::Instant;

/// Token-bucket rate limiter for per-peer event rate limiting.
///
/// Each peer gets `max_tokens` events per refill interval (burst capacity).
/// Tokens are refilled at `refill_rate` per second based on elapsed time.
///
/// # Examples
/// ```
/// use omnia_substrate::rate_limiter::RateLimiter;
///
/// let mut limiter = RateLimiter::new(200, 100);
/// let peer_id = [0u8; 32];
/// assert!(limiter.allow(&peer_id));
/// ```
pub struct RateLimiter {
    /// Maximum tokens per peer (burst capacity)
    max_tokens: u32,
    /// Tokens refilled per second
    refill_rate: u32,
    /// Per-peer token state
    buckets: HashMap<[u8; 32], TokenBucket>,
}

struct TokenBucket {
    /// Current number of tokens available.
    tokens: u32,
    /// Timestamp of the last refill.
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// # Arguments
    /// * `max_tokens` - Maximum tokens per peer (burst capacity)
    /// * `refill_rate` - Tokens refilled per second
    ///
    /// # Returns
    /// A new `RateLimiter` instance.
    pub fn new(max_tokens: u32, refill_rate: u32) -> Self {
        Self {
            max_tokens,
            refill_rate,
            buckets: HashMap::new(),
        }
    }

    /// Check if a peer is allowed to send. Returns true if allowed.
    ///
    /// Automatically refills tokens based on elapsed time since last check.
    /// If the peer has no bucket yet, one is created with full tokens.
    ///
    /// # Arguments
    /// * `peer` - The 32-byte peer identifier
    ///
    /// # Returns
    /// `true` if the peer is within rate limits, `false` if rate limited.
    pub fn allow(&mut self, peer: &[u8; 32]) -> bool {
        let now = Instant::now();

        let bucket = self.buckets.entry(*peer).or_insert_with(|| TokenBucket {
            tokens: self.max_tokens,
            last_refill: now,
        });

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(bucket.last_refill);
        let elapsed_secs = elapsed.as_secs() as u32;
        let elapsed_nanos = elapsed.subsec_nanos();
        // Fractional refill: add refill_rate * elapsed_secs + (refill_rate * elapsed_nanos / 1_000_000_000)
        let refill = elapsed_secs * self.refill_rate
            + (self.refill_rate as u64 * elapsed_nanos as u64 / 1_000_000_000) as u32;
        if refill > 0 {
            bucket.tokens = (bucket.tokens + refill).min(self.max_tokens);
            bucket.last_refill = now;
        }

        if bucket.tokens > 0 {
            bucket.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Reset a peer's bucket (e.g., after slash/ejection).
    ///
    /// Removes the peer's bucket entirely, so the next request will
    /// create a fresh bucket with full tokens.
    ///
    /// # Arguments
    /// * `peer` - The 32-byte peer identifier to reset
    pub fn reset(&mut self, peer: &[u8; 32]) {
        self.buckets.remove(peer);
    }

    /// Get the number of tracked peers.
    pub fn peer_count(&self) -> usize {
        self.buckets.len()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        // Default: 100 events/second, burst of 200
        Self::new(200, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test: peer within rate limit → events processed.
    #[test]
    fn test_peer_within_rate_limit() {
        let mut limiter = RateLimiter::new(200, 100);
        let peer_id = [1u8; 32];

        // Should allow up to 200 events (burst capacity)
        for _ in 0..200 {
            assert!(
                limiter.allow(&peer_id),
                "Should allow within burst capacity"
            );
        }
    }

    /// Test: peer exceeding rate limit → events dropped.
    #[test]
    fn test_peer_exceeding_rate_limit() {
        let mut limiter = RateLimiter::new(5, 100);
        let peer_id = [2u8; 32];

        // Exhaust the 5 tokens
        for _ in 0..5 {
            assert!(
                limiter.allow(&peer_id),
                "Should allow within burst capacity"
            );
        }

        // 6th event should be dropped
        assert!(
            !limiter.allow(&peer_id),
            "Should reject after burst capacity exhausted"
        );
    }

    /// Test: rate limit resets after refill interval.
    #[test]
    fn test_rate_limit_resets_after_refill() {
        // Very small refill rate: 1 token/sec for easier timing
        let mut limiter = RateLimiter::new(2, 1);
        let peer_id = [3u8; 32];

        // Exhaust tokens
        assert!(limiter.allow(&peer_id));
        assert!(limiter.allow(&peer_id));
        assert!(!limiter.allow(&peer_id), "Should be rate limited");

        // Wait enough for at least 1 token to refill (>1 second)
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // Should now allow one more event
        assert!(
            limiter.allow(&peer_id),
            "Should allow after refill interval"
        );
    }

    /// Test: different peers have independent rate limits.
    #[test]
    fn test_different_peers_independent() {
        let mut limiter = RateLimiter::new(3, 100);
        let peer_a = [10u8; 32];
        let peer_b = [20u8; 32];

        // Exhaust peer A's tokens
        for _ in 0..3 {
            assert!(limiter.allow(&peer_a), "Peer A within limit");
        }
        assert!(!limiter.allow(&peer_a), "Peer A exceeded limit");

        // Peer B should still have full tokens
        for _ in 0..3 {
            assert!(limiter.allow(&peer_b), "Peer B within limit");
        }
        assert!(!limiter.allow(&peer_b), "Peer B exceeded limit");
    }

    /// Test: reset clears a peer's bucket.
    #[test]
    fn test_reset_peer() {
        let mut limiter = RateLimiter::new(2, 100);
        let peer_id = [4u8; 32];

        // Exhaust tokens
        assert!(limiter.allow(&peer_id));
        assert!(limiter.allow(&peer_id));
        assert!(!limiter.allow(&peer_id));

        // Reset the peer
        limiter.reset(&peer_id);

        // Should now allow events again (fresh bucket with full tokens)
        assert!(limiter.allow(&peer_id), "Should allow after reset");
    }

    /// Test: peer_count tracks the number of peers.
    #[test]
    fn test_peer_count() {
        let mut limiter = RateLimiter::new(10, 10);
        assert_eq!(limiter.peer_count(), 0);

        let peer_a = [1u8; 32];
        let peer_b = [2u8; 32];

        limiter.allow(&peer_a);
        assert_eq!(limiter.peer_count(), 1);

        limiter.allow(&peer_b);
        assert_eq!(limiter.peer_count(), 2);

        limiter.reset(&peer_a);
        assert_eq!(limiter.peer_count(), 1);
    }

    /// Test: default rate limiter has expected values.
    #[test]
    fn test_default_rate_limiter() {
        let mut limiter = RateLimiter::default();
        let peer_id = [0u8; 32];

        // Default: 200 burst, 100/sec. Should allow first event.
        assert!(limiter.allow(&peer_id));
    }
}
