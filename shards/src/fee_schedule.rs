//! Fee schedule for shard operations
//!
//! Defines per-operation-type fees (in UBC units) for each shard domain.
//! The fee schedule is used by `ShardRouter` to deduct fees before
//! processing any shard operation, preventing spam.

use crate::payload::ShardOp;
use serde::{Deserialize, Serialize};

/// Fee schedule with per-operation-type fees (u64, in UBC units).
///
/// Each field specifies the fee (in UBC compute units) charged for
/// submitting an operation of the corresponding domain type. The
/// `cross_shard_fee` applies to any `ShardOp::CrossShard` operation,
/// and `default_fee` is the fallback for unrecognized operation types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeSchedule {
    /// Fee for financial-domain operations (transfers, mint, burn).
    pub financial_op_fee: u64,
    /// Fee for computational-domain operations (task submission, proof verification).
    pub computational_op_fee: u64,
    /// Fee for physical-domain operations (asset anchoring, ownership transfer).
    pub physical_op_fee: u64,
    /// Fee for identity-domain operations (DID lifecycle, social recovery).
    pub identity_op_fee: u64,
    /// Fee for biological-domain operations (consent management, ZK queries).
    pub biological_op_fee: u64,
    /// Fee for cross-shard messages.
    pub cross_shard_fee: u64,
    /// Default fee used as fallback for unrecognized operation types.
    pub default_fee: u64,
}

impl Default for FeeSchedule {
    fn default() -> Self {
        Self::standard()
    }
}

impl FeeSchedule {
    /// Create the standard fee schedule with production defaults.
    ///
    /// | Domain       | Fee (UBC) |
    /// |--------------|-----------|
    /// | Financial    | 10        |
    /// | Computational| 5         |
    /// | Physical     | 3         |
    /// | Identity     | 2         |
    /// | Biological   | 3         |
    /// | Cross-shard  | 15        |
    /// | Default      | 3         |
    pub fn standard() -> Self {
        Self {
            financial_op_fee: 10,
            computational_op_fee: 5,
            physical_op_fee: 3,
            identity_op_fee: 2,
            biological_op_fee: 3,
            cross_shard_fee: 15,
            default_fee: 3,
        }
    }

    /// Create a zero-fee schedule where all operations are free.
    ///
    /// Useful for testing and for backward-compatible router
    /// construction via `ShardRouter::new_without_fees()`.
    pub fn zero() -> Self {
        Self {
            financial_op_fee: 0,
            computational_op_fee: 0,
            physical_op_fee: 0,
            identity_op_fee: 0,
            biological_op_fee: 0,
            cross_shard_fee: 0,
            default_fee: 0,
        }
    }

    /// Return the fee (in UBC units) for the given shard operation type.
    ///
    /// Maps each `ShardOp` variant to its corresponding fee field.
    /// Cross-shard operations use `cross_shard_fee`; the `Economics`
    /// variant falls back to `default_fee`.
    pub fn fee_for_op(&self, op: &ShardOp) -> u64 {
        match op {
            ShardOp::Financial(_) => self.financial_op_fee,
            ShardOp::Computational(_) => self.computational_op_fee,
            ShardOp::Physical(_) => self.physical_op_fee,
            ShardOp::Identity(_) => self.identity_op_fee,
            ShardOp::Biological(_) => self.biological_op_fee,
            ShardOp::CrossShard(_) => self.cross_shard_fee,
            ShardOp::Economics(_) => self.default_fee,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_shard::CrossShardMessage;
    use crate::shard::ShardId;
    use omnia_substrate::VectorClock;

    fn make_cross_shard_msg() -> CrossShardMessage {
        CrossShardMessage::new(
            ShardId::financial(),
            ShardId::identity(),
            vec![1, 2, 3],
            VectorClock::new(),
        )
    }

    #[test]
    fn test_standard_schedule_values() {
        let schedule = FeeSchedule::standard();
        assert_eq!(schedule.financial_op_fee, 10);
        assert_eq!(schedule.computational_op_fee, 5);
        assert_eq!(schedule.physical_op_fee, 3);
        assert_eq!(schedule.identity_op_fee, 2);
        assert_eq!(schedule.biological_op_fee, 3);
        assert_eq!(schedule.cross_shard_fee, 15);
        assert_eq!(schedule.default_fee, 3);
    }

    #[test]
    fn test_zero_schedule_all_fees_zero() {
        let schedule = FeeSchedule::zero();
        assert_eq!(schedule.financial_op_fee, 0);
        assert_eq!(schedule.computational_op_fee, 0);
        assert_eq!(schedule.physical_op_fee, 0);
        assert_eq!(schedule.identity_op_fee, 0);
        assert_eq!(schedule.biological_op_fee, 0);
        assert_eq!(schedule.cross_shard_fee, 0);
        assert_eq!(schedule.default_fee, 0);
    }

    #[test]
    fn test_fee_for_op_financial() {
        let schedule = FeeSchedule::standard();
        let op = ShardOp::Financial(crate::financial::ops::FinancialOp::BalanceQuery {
            account: [0u8; 32],
        });
        assert_eq!(schedule.fee_for_op(&op), 10);
    }

    #[test]
    fn test_fee_for_op_cross_shard() {
        let schedule = FeeSchedule::standard();
        let op = ShardOp::CrossShard(make_cross_shard_msg());
        assert_eq!(schedule.fee_for_op(&op), 15);
    }

    #[test]
    fn test_fee_for_op_zero_schedule() {
        let schedule = FeeSchedule::zero();
        let op = ShardOp::Financial(crate::financial::ops::FinancialOp::BalanceQuery {
            account: [0u8; 32],
        });
        assert_eq!(schedule.fee_for_op(&op), 0);
    }

    #[test]
    fn test_fee_for_op_economics_uses_default() {
        let schedule = FeeSchedule::standard();
        let op = ShardOp::Economics(crate::economics_shard::EconomicsOp::AdvanceEpoch);
        assert_eq!(schedule.fee_for_op(&op), schedule.default_fee);
    }

    #[test]
    fn test_default_is_standard() {
        let default_schedule = FeeSchedule::default();
        let standard_schedule = FeeSchedule::standard();
        assert_eq!(
            default_schedule.financial_op_fee,
            standard_schedule.financial_op_fee
        );
        assert_eq!(
            default_schedule.computational_op_fee,
            standard_schedule.computational_op_fee
        );
        assert_eq!(
            default_schedule.cross_shard_fee,
            standard_schedule.cross_shard_fee
        );
    }
}
