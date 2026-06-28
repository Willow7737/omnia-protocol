//! Time-Locked Voting — Flash Loan Attack Prevention.
//!
//! This module implements time-locked staking and voting, preventing flash
//! loan attacks where an attacker borrows a large amount of stake, votes,
//! and repays the loan in the same transaction block.
//!
//! # How It Works
//!
//! 1. A user **locks** their stake for a minimum duration.
//! 2. Only locked stake counts as **voting power**.
//! 3. Stake cannot be unlocked until the lock duration has elapsed.
//! 4. After the lock expires, the user can **release** their stake.
//!
//! # Security Guarantees
//!
//! - **Flash loan resistance**: Borrowed funds cannot be used for voting
//!   because they must be locked for multiple blocks before voting power
//!   is granted.
//! - **Long-range attack resistance**: The lock duration ensures that
//!   validators cannot quickly re-stake after being slashed.
//! - **Fairness**: All participants must commit stake for the same minimum
//!   duration, preventing time-based manipulation.

use omnia_substrate::vector_clock::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Errors that can occur during time-locked voting operations.
#[derive(Error, Debug)]
pub enum TimeLockError {
    /// The lock duration is below the configured minimum.
    #[error("lock duration {duration} is below minimum {min_duration}")]
    DurationBelowMinimum {
        /// Requested lock duration.
        duration: u64,
        /// Minimum allowed duration.
        min_duration: u64,
    },
    /// The lock duration exceeds the configured maximum.
    #[error("lock duration {duration} exceeds maximum {max_duration}")]
    DurationExceedsMaximum {
        /// Requested lock duration.
        duration: u64,
        /// Maximum allowed duration.
        max_duration: u64,
    },
}

/// Configuration for time-locked voting.
///
/// Controls the minimum lock duration and other parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeLockConfig {
    /// Minimum duration (in blocks) that stake must be locked before it
    /// grants voting power.
    ///
    /// Default: 100 blocks. At ~5s finality, this is ~8 minutes.
    pub min_lock_duration: u64,
    /// Maximum duration (in blocks) that stake can be locked.
    ///
    /// Default: 100000 blocks. At ~5s finality, this is ~6 days.
    pub max_lock_duration: u64,
    /// Whether to enforce the lock duration strictly (no early withdrawals).
    ///
    /// Default: true. Set to false only in testing.
    /// TODO: Currently unused - enforcement is handled by `is_mature()` check.
    /// Will be used for configurable grace periods in future versions.
    #[allow(dead_code)]
    pub strict_enforcement: bool,
}

impl Default for TimeLockConfig {
    fn default() -> Self {
        Self {
            min_lock_duration: 100,
            max_lock_duration: 100_000,
            strict_enforcement: true,
        }
    }
}

/// A stake that has been locked for a specific duration.
///
/// The stake only grants voting power after `lock_start + min_lock_duration`
/// blocks have passed. Before that, the stake is locked but has no voting
/// power, preventing flash loan attacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedStake {
    /// The node that owns this locked stake.
    pub owner: NodeId,
    /// The amount of stake locked.
    pub amount: u64,
    /// The block height at which the stake was locked.
    pub lock_start: u64,
    /// The block height at which the stake can be released.
    pub lock_end: u64,
    /// Whether the stake has been released.
    pub released: bool,
}

impl LockedStake {
    /// Create a new locked stake.
    ///
    /// # Arguments
    ///
    /// * `owner` — The node locking the stake.
    /// * `amount` — The amount of stake to lock.
    /// * `lock_start` — The current block height.
    /// * `duration` — The number of blocks to lock for.
    pub fn new(owner: NodeId, amount: u64, lock_start: u64, duration: u64) -> Self {
        Self {
            owner,
            amount,
            lock_start,
            lock_end: lock_start.saturating_add(duration),
            released: false,
        }
    }

    /// Check whether this stake has matured (lock duration has elapsed).
    ///
    /// # Arguments
    ///
    /// * `current_height` — The current block height.
    pub fn is_mature(&self, current_height: u64) -> bool {
        current_height >= self.lock_end
    }

    /// Calculate the voting power of this stake at the given height.
    ///
    /// Returns the full amount if the stake is mature, zero otherwise.
    /// This ensures that freshly-locked stake (e.g., from a flash loan)
    /// cannot be used for voting.
    pub fn voting_power(&self, current_height: u64) -> u64 {
        if self.released {
            0
        } else if self.is_mature(current_height) {
            self.amount
        } else {
            0
        }
    }
}

/// Time-locked voting system.
///
/// Manages locked stakes and voting power for all participants. Enforces
/// the minimum lock duration before granting voting power.
///
/// # Example
///
/// ```
/// use omnia_economics::time_lock::{TimeLockConfig, TimeLockVoting};
///
/// let config = TimeLockConfig::default();
/// let mut voting = TimeLockVoting::new(config);
///
/// let mut node = [0u8; 32];
/// node[0] = 42;
///
/// // Lock 1000 tokens for 200 blocks starting at height 100
/// voting.lock(node, 1000, 100, 200);
///
/// // At height 250 (before maturity), no voting power
/// assert_eq!(voting.voting_power(&node, 250), 0);
///
/// // At height 300 (after maturity), full voting power
/// assert_eq!(voting.voting_power(&node, 300), 1000);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeLockVoting {
    /// Configuration for time-locked voting.
    pub config: TimeLockConfig,
    /// All locked stakes, indexed by owner.
    stakes: BTreeMap<NodeId, Vec<LockedStake>>,
}

impl TimeLockVoting {
    /// Create a new time-locked voting system with the given configuration.
    pub fn new(config: TimeLockConfig) -> Self {
        Self {
            config,
            stakes: BTreeMap::new(),
        }
    }

    /// Lock stake for a given duration.
    ///
    /// The stake is locked from `current_height` to
    /// `current_height + duration`. It only grants voting power after
    /// the lock matures (i.e., `current_height >= lock_end`).
    ///
    /// # Arguments
    ///
    /// * `owner` — The node locking the stake.
    /// * `amount` — The amount of stake to lock.
    /// * `current_height` — The current block height.
    /// * `duration` — The number of blocks to lock for.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the lock was successful, or an error string if the
    /// duration is outside the configured bounds.
    pub fn lock(
        &mut self,
        owner: NodeId,
        amount: u64,
        current_height: u64,
        duration: u64,
    ) -> Result<(), TimeLockError> {
        if duration < self.config.min_lock_duration {
            return Err(TimeLockError::DurationBelowMinimum {
                duration,
                min_duration: self.config.min_lock_duration,
            });
        }
        if duration > self.config.max_lock_duration {
            return Err(TimeLockError::DurationExceedsMaximum {
                duration,
                max_duration: self.config.max_lock_duration,
            });
        }

        let locked = LockedStake::new(owner, amount, current_height, duration);
        self.stakes.entry(owner).or_default().push(locked);

        tracing::info!(
            owner = ?&owner[..4],
            amount,
            duration,
            current_height,
            "Stake locked"
        );

        Ok(())
    }

    /// Calculate the total voting power for a node at the given height.
    ///
    /// Only mature (not released, lock duration elapsed) stakes contribute.
    pub fn voting_power(&self, owner: &NodeId, current_height: u64) -> u64 {
        self.stakes
            .get(owner)
            .map(|stakes| stakes.iter().map(|s| s.voting_power(current_height)).sum())
            .unwrap_or(0)
    }

    /// Release all expired (mature) stakes for a node.
    ///
    /// Returns the total amount released. Released stakes no longer
    /// contribute to voting power.
    ///
    /// # Arguments
    ///
    /// * `owner` — The node whose expired stakes to release.
    /// * `current_height` — The current block height.
    pub fn release_expired(&mut self, owner: &NodeId, current_height: u64) -> u64 {
        if let Some(stakes) = self.stakes.get_mut(owner) {
            let mut total_released = 0u64;
            for stake in stakes.iter_mut() {
                if !stake.released && stake.is_mature(current_height) {
                    stake.released = true;
                    total_released += stake.amount;
                }
            }
            // Compact the Vec to remove released entries, preventing unbounded growth
            stakes.retain(|s| !s.released);
            if total_released > 0 {
                tracing::info!(
                    owner = ?&owner[..4],
                    amount = total_released,
                    "Expired stake released"
                );
            }
            total_released
        } else {
            0
        }
    }

    /// Check whether a node can vote at the given height.
    ///
    /// A node can vote if it has at least some mature locked stake.
    pub fn can_vote(&self, owner: &NodeId, current_height: u64) -> bool {
        self.voting_power(owner, current_height) > 0
    }

    /// Get the total locked stake (including immature) for a node.
    pub fn total_locked(&self, owner: &NodeId) -> u64 {
        self.stakes
            .get(owner)
            .map(|stakes| stakes.iter().filter(|s| !s.released).map(|s| s.amount).sum())
            .unwrap_or(0)
    }

    /// Get the number of active (non-released) lock entries for a node.
    pub fn active_lock_count(&self, owner: &NodeId) -> usize {
        self.stakes
            .get(owner)
            .map(|stakes| stakes.iter().filter(|s| !s.released).count())
            .unwrap_or(0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    #[test]
    fn test_lock_stake() {
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(1);

        voting.lock(n, 1000, 100, 200).unwrap();
        assert_eq!(voting.total_locked(&n), 1000);
    }

    #[test]
    fn test_voting_power_before_maturity() {
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(2);

        // Lock at height 100, duration 200, matures at height 300
        voting.lock(n, 1000, 100, 200).unwrap();

        // Before maturity: no voting power
        assert_eq!(voting.voting_power(&n, 200), 0);
        assert_eq!(voting.voting_power(&n, 299), 0);
        assert!(!voting.can_vote(&n, 299));
    }

    #[test]
    fn test_voting_power_after_maturity() {
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(3);

        // Lock at height 100, duration 200, matures at height 300
        voting.lock(n, 1000, 100, 200).unwrap();

        // At maturity: full voting power
        assert_eq!(voting.voting_power(&n, 300), 1000);
        assert!(voting.can_vote(&n, 300));
        assert_eq!(voting.voting_power(&n, 500), 1000);
    }

    #[test]
    fn test_release_expired() {
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(4);

        voting.lock(n, 1000, 100, 200).unwrap();

        // Before maturity: nothing to release
        assert_eq!(voting.release_expired(&n, 250), 0);

        // After maturity: release the stake
        let released = voting.release_expired(&n, 300);
        assert_eq!(released, 1000);

        // After release: no voting power
        assert_eq!(voting.voting_power(&n, 300), 0);
        assert_eq!(voting.total_locked(&n), 0);
    }

    #[test]
    fn test_multiple_locks() {
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(5);

        voting.lock(n, 500, 100, 200).unwrap();
        voting.lock(n, 500, 150, 200).unwrap();

        // Total locked: 1000
        assert_eq!(voting.total_locked(&n), 1000);

        // First lock matures at 300, second at 350
        assert_eq!(voting.voting_power(&n, 310), 500);
        assert_eq!(voting.voting_power(&n, 350), 1000);
    }

    #[test]
    fn test_lock_duration_below_minimum() {
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(6);

        let result = voting.lock(n, 1000, 100, 50);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TimeLockError::DurationBelowMinimum {
                duration: 50,
                min_duration: 100,
            }
        ));
    }

    #[test]
    fn test_lock_duration_exceeds_maximum() {
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(7);

        let result = voting.lock(n, 1000, 100, 200_000);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TimeLockError::DurationExceedsMaximum {
                duration: 200_000,
                max_duration: 100_000,
            }
        ));
    }

    #[test]
    fn test_no_stake_no_power() {
        let config = TimeLockConfig::default();
        let voting = TimeLockVoting::new(config);
        let n = node(99);

        assert_eq!(voting.voting_power(&n, 1000), 0);
        assert!(!voting.can_vote(&n, 1000));
        assert_eq!(voting.total_locked(&n), 0);
    }

    #[test]
    fn test_default_config() {
        let config = TimeLockConfig::default();
        assert_eq!(config.min_lock_duration, 100);
        assert_eq!(config.max_lock_duration, 100_000);
        assert!(config.strict_enforcement);
    }

    #[test]
    fn test_active_lock_count() {
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(8);

        assert_eq!(voting.active_lock_count(&n), 0);
        voting.lock(n, 500, 100, 200).unwrap();
        voting.lock(n, 500, 150, 200).unwrap();
        assert_eq!(voting.active_lock_count(&n), 2);
    }

    // --- Spec-required named tests ---

    #[test]
    fn test_lock_creates_eligible_stake() {
        // Verify that locking creates a stake eligible for voting after
        // the lock duration elapses.
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(10);

        // Lock 1000 tokens at height 100, duration 200 -> eligible at height 300
        voting.lock(n, 1000, 100, 200).unwrap();

        // Before maturity: not eligible
        assert!(!voting.can_vote(&n, 299));

        // After maturity: eligible
        assert!(voting.can_vote(&n, 300));
        assert_eq!(voting.voting_power(&n, 300), 1000);
    }

    #[test]
    fn test_flash_loan_prevented() {
        // Flash loan attack: an attacker borrows stake, votes, and repays
        // in the same block. Time-locking prevents this because newly-locked
        // stake has zero voting power until the lock matures.
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let attacker = node(20);

        // Attacker locks at height 500 (simulating a flash loan)
        voting.lock(attacker, 1_000_000, 500, 200).unwrap();

        // Same block: no voting power (flash loan prevented!)
        assert_eq!(voting.voting_power(&attacker, 500), 0);
        assert!(!voting.can_vote(&attacker, 500));

        // Even several blocks later (but before maturity): still no power
        assert_eq!(voting.voting_power(&attacker, 600), 0);
    }

    #[test]
    fn test_multiple_locks_accumulate() {
        // Multiple locks from the same account should accumulate voting power.
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(30);

        // Lock 300 tokens at height 100, duration 200 -> matures at 300
        voting.lock(n, 300, 100, 200).unwrap();

        // Lock 700 tokens at height 100, duration 300 -> matures at 400
        voting.lock(n, 700, 100, 300).unwrap();

        // At height 300: only first lock matured -> 300 power
        assert_eq!(voting.voting_power(&n, 300), 300);

        // At height 400: both locks matured -> 1000 power
        assert_eq!(voting.voting_power(&n, 400), 1000);

        // Total locked (including immature) is 1000
        assert_eq!(voting.total_locked(&n), 1000);
    }

    #[test]
    fn test_expired_locks_released() {
        // After locks mature and are released, voting power drops to zero.
        let config = TimeLockConfig::default();
        let mut voting = TimeLockVoting::new(config);
        let n = node(40);

        voting.lock(n, 1000, 100, 200).unwrap();

        // At maturity: has power
        assert_eq!(voting.voting_power(&n, 300), 1000);

        // Release the expired lock
        let released = voting.release_expired(&n, 300);
        assert_eq!(released, 1000);

        // After release: no power, no locked stake
        assert_eq!(voting.voting_power(&n, 300), 0);
        assert_eq!(voting.total_locked(&n), 0);
    }

    #[test]
    fn test_time_lock_error_variants_display() {
        let e = TimeLockError::DurationBelowMinimum {
            duration: 50,
            min_duration: 100,
        };
        assert!(e.to_string().contains("50"));
        assert!(e.to_string().contains("100"));
        assert!(e.to_string().contains("below minimum"));

        let e = TimeLockError::DurationExceedsMaximum {
            duration: 200_000,
            max_duration: 100_000,
        };
        assert!(e.to_string().contains("200000"));
        assert!(e.to_string().contains("100000"));
        assert!(e.to_string().contains("exceeds maximum"));
    }
}
