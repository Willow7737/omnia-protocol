//! Computational shard state
//!
//! Maintains the task queue and proof registry. Tasks progress through
//! a lifecycle: Submitted → Proved → Verified.

use std::collections::BTreeMap;

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
    pub tasks: BTreeMap<super::ops::TaskId, TaskEntry>,
}

impl ComputationalState {
    /// Create an empty computational state.
    pub fn new() -> Self {
        Self { tasks: BTreeMap::new() }
    }

    /// Apply a computational operation, mutating state.
    pub fn apply(&mut self, op: &ComputationalOp, vc: &VectorClock) -> Result<(), ShardError> {
        match op {
            ComputationalOp::SubmitTask { task_id, spec, reward } => {
                if self.tasks.contains_key(task_id) {
                    return Err(ShardError::StateConflict(format!("Task already exists: {task_id:?}")));
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
                    return Err(ShardError::InvalidOperation("Only Proved tasks can be verified".into()));
                }

                #[allow(unused_variables)] // Used only when `real_verification` feature is enabled
                let proof_bytes = task
                    .proof
                    .as_ref()
                    .ok_or_else(|| ShardError::ValidationFailed("No proof data available for verification".into()))?;

                // -----------------------------------------------------------------------
                // Real ZK/SNARK proof verification using ark-groth16.
                // Enabled via the `real_verification` feature flag.
                // -----------------------------------------------------------------------
                #[cfg(feature = "real_verification")]
                {
                    // AUDIT-2026-07 C9 (#347): the verifying key comes from
                    // the node's VK registry, never from the caller. The
                    // submission is `[32-byte circuit ID || proof]`; an
                    // unregistered circuit ID is rejected outright. The
                    // single public input binds the proof to this exact
                    // task (task_id + spec), so a proof for one task
                    // cannot be replayed against another. This also
                    // replaces the previous hardcoded-empty public-input
                    // list, which rejected every submission.
                    let public_inputs = vec![crate::zk::groth16::computational_public_input(task_id, &task.spec)];
                    match crate::zk::groth16::verify_with_registry(proof_bytes, &public_inputs, "computational task") {
                        Ok(()) => {
                            tracing::info!(
                                task = ?&task_id[..4],
                                "Real ZK verification: proof verified successfully"
                            );
                            task.status = TaskStatus::Verified;
                            task.last_update.merge(vc);
                            Ok(())
                        }
                        Err(e) => {
                            tracing::warn!(
                                task = ?&task_id[..4],
                                error = %e,
                                "Real ZK verification: proof rejected"
                            );
                            task.status = TaskStatus::Failed;
                            task.last_update.merge(vc);
                            Err(e)
                        }
                    }
                }

                // When real_verification is disabled, always reject
                #[cfg(not(feature = "real_verification"))]
                {
                    task.status = TaskStatus::Failed;
                    task.last_update.merge(vc);
                    Err(ShardError::ValidationFailed(
                        "ZK proof verification requires 'real_verification' feature to be enabled".into(),
                    ))
                }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_substrate::VectorClock;

    /// Helper: create a VectorClock with a single node at counter 1.
    fn test_vc() -> VectorClock {
        VectorClock::with_node([1u8; 32], 1)
    }

    #[test]
    fn test_malformed_proof_rejected() {
        let mut state = ComputationalState::new();
        let vc = test_vc();
        let task_id = [0xAB; 32];

        // 1. Submit a task
        state
            .apply(
                &ComputationalOp::SubmitTask {
                    task_id,
                    spec: vec![1, 2, 3],
                    reward: 100,
                },
                &vc,
            )
            .expect("test assertion failed");

        // 2. Submit a 1-byte (malformed) proof
        state
            .apply(
                &ComputationalOp::SubmitProof {
                    task_id,
                    proof: vec![0xFF],
                },
                &vc,
            )
            .expect("test assertion failed");

        // 3. Verify the 1-byte proof — should fail without real_verification
        let result = state.apply(&ComputationalOp::VerifyProof { task_id }, &vc);
        assert!(result.is_err(), "ZK proof should be rejected without real_verification");
        match result.expect_err("test assertion failed") {
            ShardError::ValidationFailed(msg) => {
                assert!(
                    msg.contains("real_verification") || msg.contains("too short"),
                    "expected real_verification or too-short-submission error, got: {msg}"
                );
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }

        // Task should be in Failed status
        assert_eq!(state.tasks[&task_id].status, TaskStatus::Failed);
    }

    #[test]
    fn test_empty_proof_rejected() {
        let mut state = ComputationalState::new();
        let vc = test_vc();
        let task_id = [0xCD; 32];

        // Submit a task
        state
            .apply(
                &ComputationalOp::SubmitTask {
                    task_id,
                    spec: vec![1, 2, 3],
                    reward: 100,
                },
                &vc,
            )
            .expect("test assertion failed");

        // Submit an empty proof
        state
            .apply(&ComputationalOp::SubmitProof { task_id, proof: vec![] }, &vc)
            .expect("test assertion failed");

        // Verify the empty proof — should fail
        let result = state.apply(&ComputationalOp::VerifyProof { task_id }, &vc);
        assert!(result.is_err(), "ZK proof should be rejected without real_verification");
        match result.expect_err("test assertion failed") {
            ShardError::ValidationFailed(msg) => {
                assert!(
                    msg.contains("real_verification") || msg.contains("too short"),
                    "expected real_verification or too-short-submission error, got: {msg}"
                );
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_valid_layout_proof_also_rejected_without_real_verification() {
        let mut state = ComputationalState::new();
        let vc = test_vc();
        let task_id = [0xEF; 32];

        // Submit a task
        state
            .apply(
                &ComputationalOp::SubmitTask {
                    task_id,
                    spec: vec![1, 2, 3],
                    reward: 100,
                },
                &vc,
            )
            .expect("test assertion failed");

        // Submit a well-formed (128+ byte) proof
        state
            .apply(
                &ComputationalOp::SubmitProof {
                    task_id,
                    proof: vec![0u8; 192],
                },
                &vc,
            )
            .expect("test assertion failed");

        // Verify — should STILL fail because real_verification is not enabled
        let result = state.apply(&ComputationalOp::VerifyProof { task_id }, &vc);
        assert!(result.is_err(), "ZK proof should be rejected without real_verification");
        assert_eq!(state.tasks[&task_id].status, TaskStatus::Failed);
    }
}
