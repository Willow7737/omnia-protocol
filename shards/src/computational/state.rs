//! Computational shard state
//!
//! Maintains the task queue and proof registry. Tasks progress through
//! a lifecycle: Submitted → Proved → Verified.

use std::collections::HashMap;

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

use super::ops::ComputationalOp;
use crate::shard::ShardError;

/// Status of a compute task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task has been submitted but no proof yet.
    Submitted,
    /// A proof has been submitted but not yet verified.
    Proved,
    /// The proof has been verified successfully.
    Verified,
    /// The proof failed verification.
    Failed,
}

/// A compute task entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEntry {
    /// Unique task identifier.
    pub task_id: super::ops::TaskId,
    /// The compute specification.
    pub spec: Vec<u8>,
    /// Reward for completing the task.
    pub reward: u64,
    /// Current status.
    pub status: TaskStatus,
    /// Submitted proof data, if any.
    pub proof: Option<Vec<u8>>,
    /// Vector clock at the time of the last status change.
    pub last_update: VectorClock,
}

/// The full state of the Computational shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputationalState {
    /// Task registry — maps task IDs to task entries.
    pub tasks: HashMap<super::ops::TaskId, TaskEntry>,
}

impl ComputationalState {
    /// Create an empty computational state.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    /// Apply a computational operation, mutating state.
    pub fn apply(&mut self, op: &ComputationalOp, vc: &VectorClock) -> Result<(), ShardError> {
        match op {
            ComputationalOp::SubmitTask {
                task_id,
                spec,
                reward,
            } => {
                if self.tasks.contains_key(task_id) {
                    return Err(ShardError::StateConflict(format!(
                        "Task already exists: {:?}",
                        task_id
                    )));
                }
                self.tasks.insert(
                    *task_id,
                    TaskEntry {
                        task_id: *task_id,
                        spec: spec.clone(),
                        reward: *reward,
                        status: TaskStatus::Submitted,
                        proof: None,
                        last_update: vc.clone(),
                    },
                );
                Ok(())
            }
            ComputationalOp::SubmitProof { task_id, proof } => {
                let task = self
                    .tasks
                    .get_mut(task_id)
                    .ok_or_else(|| ShardError::ValidationFailed("Task not found".into()))?;

                if task.status != TaskStatus::Submitted {
                    return Err(ShardError::InvalidOperation(
                        "Proof can only be submitted for tasks in Submitted status".into(),
                    ));
                }

                task.proof = Some(proof.clone());
                task.status = TaskStatus::Proved;
                task.last_update.merge(vc);
                Ok(())
            }
            ComputationalOp::VerifyProof { task_id } => {
                let task = self
                    .tasks
                    .get_mut(task_id)
                    .ok_or_else(|| ShardError::ValidationFailed("Task not found".into()))?;

                if task.status != TaskStatus::Proved {
                    return Err(ShardError::InvalidOperation(
                        "Only Proved tasks can be verified".into(),
                    ));
                }

                // In a real implementation, this would invoke a ZK-proof verifier.
                // For now, we accept the proof as valid.
                task.status = TaskStatus::Verified;
                task.last_update.merge(vc);
                Ok(())
            }
        }
    }

    /// Serialize the state to bytes for snapshots.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    /// Deserialize state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

impl Default for ComputationalState {
    fn default() -> Self {
        Self::new()
    }
}
