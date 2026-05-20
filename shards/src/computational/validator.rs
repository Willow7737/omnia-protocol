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
                    return Err(ShardError::StateConflict(format!(
                        "Task already exists: {task_id:?}"
                    )));
                }
                Ok(())
            }
            ComputationalOp::SubmitProof { task_id, .. } => match state.tasks.get(task_id) {
                Some(task) if task.status == super::state::TaskStatus::Submitted => Ok(()),
                Some(_) => Err(ShardError::InvalidOperation(
                    "Task is not in Submitted status".into(),
                )),
                None => Err(ShardError::ValidationFailed("Task not found".into())),
            },
            ComputationalOp::VerifyProof { task_id } => match state.tasks.get(task_id) {
                Some(task) if task.status == super::state::TaskStatus::Proved => Ok(()),
                Some(_) => Err(ShardError::InvalidOperation(
                    "Task is not in Proved status".into(),
                )),
                None => Err(ShardError::ValidationFailed("Task not found".into())),
            },
        }
    }

    /// Validate a `ShardOp::Computational` variant.
    pub fn validate_shard_op(state: &ComputationalState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Computational(comp_op) => Self::validate(state, comp_op),
            _ => Err(ShardError::InvalidOperation(
                "Not a Computational operation".into(),
            )),
        }
    }
}
