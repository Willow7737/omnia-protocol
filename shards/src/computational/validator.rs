//! Computational shard validator
//!
//! Pre-flight validation for computational operations.

use super::ops::ComputationalOp;
use super::state::ComputationalState;
use crate::payload::ShardOp;
use crate::shard::ShardError;

/// Validator for the Computational shard.
pub struct ComputationalValidator;

impl ComputationalValidator {
    /// Validate a computational operation against the given state.
    pub fn validate(state: &ComputationalState, op: &ComputationalOp) -> Result<(), ShardError> {
        match op {
            ComputationalOp::SubmitTask { task_id, .. } => {
                if state.tasks.contains_key(task_id) {
                    return Err(ShardError::StateConflict(format!("Task already exists: {task_id:?}")));
                }
                Ok(())
            }
            ComputationalOp::SubmitProof { task_id, .. } => match state.tasks.get(task_id) {
                Some(task) if task.status == super::state::TaskStatus::Submitted => Ok(()),
                Some(_) => Err(ShardError::InvalidOperation("Task is not in Submitted status".into())),
                None => Err(ShardError::ValidationFailed("Task not found".into())),
            },
            ComputationalOp::VerifyProof { task_id } => match state.tasks.get(task_id) {
                Some(task) if task.status == super::state::TaskStatus::Proved => Ok(()),
                Some(_) => Err(ShardError::InvalidOperation("Task is not in Proved status".into())),
                None => Err(ShardError::ValidationFailed("Task not found".into())),
            },
        }
    }

    /// Validate a `ShardOp::Computational` variant.
    pub fn validate_shard_op(state: &ComputationalState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Computational(comp_op) => Self::validate(state, comp_op),
            _ => Err(ShardError::InvalidOperation("Not a Computational operation".into())),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::payload::ShardOp;

    fn task_id() -> TaskId {
        [0xCC; 32]
    }

    #[test]
    fn test_submit_task_new_accepted() {
        let state = ComputationalState::new();
        let op = ComputationalOp::SubmitTask {
            task_id: task_id(),
            spec: vec![1, 2, 3],
            reward: 100,
        };
        assert!(ComputationalValidator::validate(&state, &op).is_ok());
    }

    #[test]
    fn test_submit_task_duplicate_rejected() {
        let mut state = ComputationalState::new();
        // Manually insert a task to simulate an existing entry
        state.tasks.insert(
            task_id(),
            super::super::state::TaskEntry {
                task_id: task_id(),
                spec: vec![],
                reward: 0,
                status: super::super::state::TaskStatus::Submitted,
                proof: None,
                last_update: omnia_substrate::VectorClock::new(),
            },
        );
        let op = ComputationalOp::SubmitTask {
            task_id: task_id(),
            spec: vec![1, 2, 3],
            reward: 100,
        };
        let result = ComputationalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::StateConflict(msg) => assert!(msg.contains("Task already exists")),
            other => panic!("Expected StateConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_submit_proof_task_not_found_rejected() {
        let state = ComputationalState::new();
        let op = ComputationalOp::SubmitProof {
            task_id: task_id(),
            proof: vec![0xAA],
        };
        let result = ComputationalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("Task not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_proof_task_not_found_rejected() {
        let state = ComputationalState::new();
        let op = ComputationalOp::VerifyProof { task_id: task_id() };
        let result = ComputationalValidator::validate(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::ValidationFailed(msg) => assert!(msg.contains("Task not found")),
            other => panic!("Expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_shard_op_wrong_variant_rejected() {
        let state = ComputationalState::new();
        let op = ShardOp::Financial(crate::FinancialOp::BalanceQuery { account: [0u8; 32] });
        let result = ComputationalValidator::validate_shard_op(&state, &op);
        assert!(result.is_err());
        match result.unwrap_err() {
            ShardError::InvalidOperation(msg) => assert!(msg.contains("Not a Computational")),
            other => panic!("Expected InvalidOperation, got {other:?}"),
        }
    }
}
