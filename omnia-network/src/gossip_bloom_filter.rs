//! Bloom filter for duplicate event suppression
//!
//! Uses a rotating bloom filter pair to suppress duplicate events
//! at the gossip layer. When an event is seen, its hash is added
//! to the current filter. Before propagating, we check if the event
//! was already seen.
//!
//! The filter pair rotates periodically to allow old entries to
//! expire, preventing false positives from accumulating indefinitely.
//!
//! # Implementation
//!
//! The bloom filter is implemented from scratch using BLAKE3 for
//! hashing (no additional dependencies). Multiple hash functions
//! are derived by hashing with different seed suffixes, which is
//! a standard technique for bloom filters with cryptographic hash
//! functions.
//!
//! # False Positive Rate
//!
//! The false positive rate (FPR) is determined by:
//! - Number of bits in the filter (m)
//! - Number of hash functions (k)
//! - Number of inserted items (n)
//!
//! FPR ≈ (1 - e^(-kn/m))^k
//!
//! For the default configuration (100,000 items, FPR 0.001):
//! - m ≈ 1,437,759 bits (~175 KiB)
//! - k ≈ 10 hash functions

use omnia_primitives::EventId;

/// Number of hash functions used by default.
const DEFAULT_NUM_HASHES: usize = 10;

/// A single bloom filter instance.
///
/// Uses a bit vector and multiple BLAKE3-derived hash functions.
#[derive(Clone, Debug)]
struct BloomFilter {
    /// Bit vector stored as bytes.
    bits: Vec<u8>,
    /// Number of bits in the filter.
    num_bits: usize,
    /// Number of hash functions.
    num_hashes: usize,
    /// Number of items inserted.
    count: usize,
}

impl BloomFilter {
    /// Create a new bloom filter with the given number of bits and hash functions.
    fn new(num_bits: usize, num_hashes: usize) -> Self {
        let num_bytes = num_bits.div_ceil(8);
        Self {
            bits: vec![0u8; num_bytes],
            num_bits,
            num_hashes,
            count: 0,
        }
    }

    /// Insert an item into the filter.
    fn insert(&mut self, item: &[u8]) {
        let hashes = self.hash_offsets(item);
        for offset in hashes {
            let byte_idx = offset / 8;
            let bit_idx = offset % 8;
            self.bits[byte_idx] |= 1 << bit_idx;
        }
        self.count += 1;
    }

    /// Check if an item might be in the filter.
    ///
    /// Returns `true` if the item might be present (may be a false positive).
    /// Returns `false` if the item is definitely not present.
    fn might_contain(&self, item: &[u8]) -> bool {
        let hashes = self.hash_offsets(item);
        for offset in hashes {
            let byte_idx = offset / 8;
            let bit_idx = offset % 8;
            if self.bits[byte_idx] & (1 << bit_idx) == 0 {
                return false;
            }
        }
        true
    }

    /// Clear the filter, removing all entries.
    fn clear(&mut self) {
        for byte in &mut self.bits {
            *byte = 0;
        }
        self.count = 0;
    }

    /// Number of items inserted.
    fn count(&self) -> usize {
        self.count
    }

    /// Compute hash offsets for an item using BLAKE3 with different seeds.
    ///
    /// Each hash function i is: blake3(item || i as u64_le) mod num_bits
    /// This derives k independent hash functions from a single hash function.
    fn hash_offsets(&self, item: &[u8]) -> Vec<usize> {
        let mut offsets = Vec::with_capacity(self.num_hashes);
        for i in 0u64..self.num_hashes as u64 {
            // Hash: blake3(item || i_le_bytes)
            let mut hasher = blake3::Hasher::new();
            hasher.update(item);
            hasher.update(&i.to_le_bytes());
            let hash = hasher.finalize();
            // Use the first 8 bytes as a u64 and mod by num_bits
            let mut hash_bytes = [0u8; 8];
            hash_bytes.copy_from_slice(&hash.as_bytes()[..8]);
            let hash_val = u64::from_le_bytes(hash_bytes);
            offsets.push((hash_val % self.num_bits as u64) as usize);
        }
        offsets
    }

    /// Estimate the current false positive rate.
    fn estimated_fpr(&self) -> f64 {
        if self.num_bits == 0 || self.count == 0 {
            return 0.0;
        }
        // FPR ≈ (1 - e^(-kn/m))^k
        let k = self.num_hashes as f64;
        let n = self.count as f64;
        let m = self.num_bits as f64;
        let exponent = -k * n / m;
        let base = 1.0 - exponent.exp();
        base.powf(k)
    }

    /// Estimated memory usage in bytes.
    fn estimated_size(&self) -> usize {
        self.bits.len()
    }
}

/// Rotating bloom filter pair for duplicate event suppression.
///
/// Two bloom filters are maintained:
/// - **Active**: New inserts go here. Checks are done against both filters.
/// - **Inactive**: Used for lookups only. Periodically cleared on rotation.
///
/// Rotation swaps the active and inactive filters and clears the new
/// inactive filter. This allows old entries to expire naturally, preventing
/// false positives from accumulating indefinitely.
///
/// # Example
///
/// ```ignore
/// use omnia_network::gossip_bloom_filter::GossipBloomFilter;
///
/// let mut filter = GossipBloomFilter::new(100_000, 0.001);
///
/// let event_id = [1u8; 32];
/// assert!(!filter.contains(&event_id));
/// filter.insert(&event_id);
/// assert!(filter.contains(&event_id));
/// ```
#[derive(Clone, Debug)]
pub struct GossipBloomFilter {
    /// Currently active filter (new inserts go here).
    active: BloomFilter,
    /// Inactive filter (lookups only, periodically cleared).
    inactive: BloomFilter,
    /// Expected number of items before rotation.
    expected_items: usize,
    /// Target false positive rate.
    target_fp_rate: f64,
}

impl GossipBloomFilter {
    /// Create a new rotating bloom filter pair.
    ///
    /// # Arguments
    ///
    /// * `expected_items` — Expected number of events seen before rotation.
    /// * `fp_rate` — Target false positive rate (e.g., 0.001 for 0.1%).
    ///
    /// The filter size and number of hash functions are calculated
    /// automatically based on these parameters.
    pub fn new(expected_items: usize, fp_rate: f64) -> Self {
        let (num_bits, num_hashes) = Self::calculate_parameters(expected_items, fp_rate);
        let active = BloomFilter::new(num_bits, num_hashes);
        let inactive = BloomFilter::new(num_bits, num_hashes);
        Self {
            active,
            inactive,
            expected_items,
            target_fp_rate: fp_rate,
        }
    }

    /// Insert an event ID, marking it as seen.
    pub fn insert(&mut self, event_id: &EventId) {
        self.active.insert(event_id);
    }

    /// Check if an event ID has been seen.
    ///
    /// Returns `true` if the event might have been seen (may be a false positive).
    /// Returns `false` if the event was definitely not seen.
    pub fn contains(&self, event_id: &EventId) -> bool {
        self.active.might_contain(event_id) || self.inactive.might_contain(event_id)
    }

    /// Rotate the filter pair.
    ///
    /// Swaps active and inactive filters, then clears the new inactive filter.
    /// This allows old entries to expire, preventing false positives from
    /// accumulating indefinitely.
    ///
    /// Call this periodically (e.g., every 300 seconds) to maintain a
    /// bounded false positive rate.
    pub fn rotate(&mut self) {
        // Swap active and inactive
        std::mem::swap(&mut self.active, &mut self.inactive);
        // Clear the new active filter (which was the old inactive).
        // Events from the old inactive have now expired (they survived
        // one rotation period in the inactive filter).
        // The new inactive (old active) retains its events so they remain
        // available for lookups for one more rotation period.
        self.active.clear();
    }

    /// Get the current estimated false positive rate.
    ///
    /// This is an upper bound based on the combined items in both filters.
    pub fn false_positive_rate(&self) -> f64 {
        let active_fpr = self.active.estimated_fpr();
        let inactive_fpr = self.inactive.estimated_fpr();
        // Combined FPR: P(fp in active) + P(fp in inactive) - P(fp in both)
        // Since the filters are independent, we approximate:
        // FPR ≈ 1 - (1 - active_fpr) * (1 - inactive_fpr)
        1.0 - (1.0 - active_fpr) * (1.0 - inactive_fpr)
    }

    /// Get the estimated memory usage in bytes.
    pub fn estimated_size(&self) -> usize {
        self.active.estimated_size() + self.inactive.estimated_size()
    }

    /// Get the number of items in the active filter.
    pub fn active_count(&self) -> usize {
        self.active.count()
    }

    /// Get the number of items in the inactive filter.
    pub fn inactive_count(&self) -> usize {
        self.inactive.count()
    }

    /// Get the total number of items across both filters.
    pub fn total_count(&self) -> usize {
        self.active.count() + self.inactive.count()
    }

    /// Get the expected number of items before rotation.
    pub fn expected_items(&self) -> usize {
        self.expected_items
    }

    /// Get the target false positive rate.
    pub fn target_fp_rate(&self) -> f64 {
        self.target_fp_rate
    }

    /// Calculate optimal bloom filter parameters.
    ///
    /// Returns `(num_bits, num_hashes)` optimized for the given
    /// expected number of items and target false positive rate.
    ///
    /// Formulas:
    /// - m = -n * ln(p) / (ln(2))^2
    /// - k = (m/n) * ln(2)
    fn calculate_parameters(expected_items: usize, fp_rate: f64) -> (usize, usize) {
        if expected_items == 0 || fp_rate <= 0.0 || fp_rate >= 1.0 {
            return (DEFAULT_NUM_HASHES * 8, DEFAULT_NUM_HASHES);
        }

        let n = expected_items as f64;
        let p = fp_rate;

        // m = -n * ln(p) / (ln(2))^2
        let ln2 = std::f64::consts::LN_2;
        let m = (-n * p.ln()) / (ln2 * ln2);
        let num_bits = m.ceil() as usize;

        // k = (m/n) * ln(2)
        let k = (m / n) * ln2;
        let num_hashes = k.ceil() as usize;

        // Ensure minimum values
        let num_bits = num_bits.max(64);
        let num_hashes = num_hashes.clamp(1, 32);

        (num_bits, num_hashes)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_bloom_filter_insert_contains() {
        let mut filter = BloomFilter::new(1024, 3);
        let item = [1u8; 32];

        assert!(!filter.might_contain(&item));
        filter.insert(&item);
        assert!(filter.might_contain(&item));
    }

    #[test]
    fn test_bloom_filter_clear() {
        let mut filter = BloomFilter::new(1024, 3);
        let item = [1u8; 32];

        filter.insert(&item);
        assert!(filter.might_contain(&item));

        filter.clear();
        assert!(!filter.might_contain(&item));
        assert_eq!(filter.count(), 0);
    }

    #[test]
    fn test_bloom_filter_no_false_negatives() {
        let mut filter = BloomFilter::new(10000, 5);

        // Insert many items
        for i in 0..100u8 {
            let mut item = [0u8; 32];
            item[0] = i;
            filter.insert(&item);
        }

        // All inserted items must be found
        for i in 0..100u8 {
            let mut item = [0u8; 32];
            item[0] = i;
            assert!(filter.might_contain(&item), "False negative for item {i}");
        }
    }

    #[test]
    fn test_bloom_filter_fpr_within_bounds() {
        // Use parameters for 1000 items at 0.01 FPR
        let (num_bits, num_hashes) = GossipBloomFilter::calculate_parameters(1000, 0.01);
        let mut filter = BloomFilter::new(num_bits, num_hashes);

        // Insert 1000 items
        for i in 0..1000u32 {
            let mut item = [0u8; 32];
            item[..4].copy_from_slice(&i.to_le_bytes());
            filter.insert(&item);
        }

        // Check FPR with items that were never inserted
        let mut false_positives = 0usize;
        let test_count = 10000usize;
        for i in 1000..1000 + test_count as u32 {
            let mut item = [0u8; 32];
            item[..4].copy_from_slice(&i.to_le_bytes());
            if filter.might_contain(&item) {
                false_positives += 1;
            }
        }

        let observed_fpr = false_positives as f64 / test_count as f64;
        // Allow 5x the target FPR as tolerance (bloom filters have variance)
        assert!(
            observed_fpr < 0.05,
            "FPR too high: {observed_fpr:.4} (target: 0.01, tolerance: 0.05)"
        );
    }

    #[test]
    fn test_gossip_bloom_filter_new() {
        let filter = GossipBloomFilter::new(100_000, 0.001);
        assert_eq!(filter.expected_items(), 100_000);
        assert!((filter.target_fp_rate() - 0.001).abs() < f64::EPSILON);
        assert_eq!(filter.active_count(), 0);
        assert_eq!(filter.inactive_count(), 0);
    }

    #[test]
    fn test_gossip_bloom_filter_insert_contains() {
        let mut filter = GossipBloomFilter::new(100_000, 0.001);
        let event_id = [42u8; 32];

        assert!(!filter.contains(&event_id));
        filter.insert(&event_id);
        assert!(filter.contains(&event_id));
    }

    #[test]
    fn test_gossip_bloom_filter_rotate() {
        let mut filter = GossipBloomFilter::new(100_000, 0.001);

        // Insert items into active filter
        let event_id = [1u8; 32];
        filter.insert(&event_id);
        assert_eq!(filter.active_count(), 1);

        // Rotate: active becomes inactive, active is cleared
        filter.rotate();
        assert_eq!(filter.active_count(), 0);
        assert_eq!(filter.inactive_count(), 1);

        // The event should still be found (in the now-inactive filter)
        assert!(filter.contains(&event_id));

        // Insert a new item after rotation
        let event_id_2 = [2u8; 32];
        filter.insert(&event_id_2);
        assert!(filter.contains(&event_id_2));

        // Both old and new items should be found
        assert!(filter.contains(&event_id));
        assert!(filter.contains(&event_id_2));
    }

    #[test]
    fn test_gossip_bloom_filter_rotate_expiration() {
        let mut filter = GossipBloomFilter::new(100_000, 0.001);

        // Insert an item
        let event_id = [1u8; 32];
        filter.insert(&event_id);

        // First rotation: active -> inactive
        filter.rotate();
        assert!(filter.contains(&event_id));

        // Second rotation: inactive (with our item) gets cleared,
        // active (which was empty) becomes inactive
        filter.rotate();
        // The event was in the inactive filter which was cleared
        assert!(!filter.contains(&event_id));
    }

    #[test]
    fn test_gossip_bloom_filter_false_positive_rate() {
        let filter = GossipBloomFilter::new(100_000, 0.001);
        // Empty filter should have 0 FPR
        let fpr = filter.false_positive_rate();
        assert_eq!(fpr, 0.0, "Empty filter should have 0 FPR");
    }

    #[test]
    fn test_gossip_bloom_filter_estimated_size() {
        let filter = GossipBloomFilter::new(100_000, 0.001);
        let size = filter.estimated_size();
        // Should be roughly 2 * 175 KiB ≈ 350 KiB
        assert!(
            size > 100_000,
            "Expected at least 100KB for 100K items at 0.001 FPR, got {size}"
        );
    }

    #[test]
    fn test_calculate_parameters_zero_items() {
        let (bits, hashes) = GossipBloomFilter::calculate_parameters(0, 0.01);
        assert!(bits > 0);
        assert!(hashes > 0);
    }

    #[test]
    fn test_calculate_parameters_edge_cases() {
        // Invalid FPR should still produce valid parameters
        let (bits, hashes) = GossipBloomFilter::calculate_parameters(1000, 0.0);
        assert!(bits > 0);
        assert!(hashes > 0);

        let (bits, hashes) = GossipBloomFilter::calculate_parameters(1000, 1.0);
        assert!(bits > 0);
        assert!(hashes > 0);
    }

    #[test]
    fn test_gossip_bloom_filter_no_false_negatives() {
        let mut filter = GossipBloomFilter::new(1000, 0.01);

        // Insert many event IDs
        let mut ids = Vec::new();
        for i in 0..100u32 {
            let mut id = [0u8; 32];
            id[..4].copy_from_slice(&i.to_le_bytes());
            filter.insert(&id);
            ids.push(id);
        }

        // All inserted IDs must be found
        for id in &ids {
            assert!(filter.contains(id), "False negative for event ID {:?}", &id[..4]);
        }
    }

    #[test]
    fn test_total_count() {
        let mut filter = GossipBloomFilter::new(100_000, 0.001);
        assert_eq!(filter.total_count(), 0);

        filter.insert(&[1u8; 32]);
        filter.insert(&[2u8; 32]);
        assert_eq!(filter.total_count(), 2);

        filter.rotate();
        filter.insert(&[3u8; 32]);
        assert_eq!(filter.total_count(), 3);
    }
}
