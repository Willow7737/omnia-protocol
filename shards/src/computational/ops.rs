//! Computational shard operations
//!
//! Defines operations for submitting compute tasks, submitting proofs of
//! computation, and verifying those proofs.

use serde::{Deserialize, Serialize};

/// Unique identifier for a compute task.
pub type TaskId = [u8; 32];

/// Operations supported by the Computational shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputationalOp {
    /// Submit a new compute task to the task queue.
    SubmitTask {
        /// Unique task identifier.
        task_id: TaskId,
        /// The compute specification (e.g., model hash, input data hash).
        spec: Vec<u8>,
        /// Reward offered for completing this task.
        reward: u64,
    },
    /// Submit a proof of computation for a previously submitted task.
    SubmitProof {
        /// The task this proof is for.
        task_id: TaskId,
        /// The proof data (format depends on the proof system).
        proof: Vec<u8>,
    },
    /// Verify a submitted proof.
    VerifyProof {
        /// The task whose proof should be verified.
        task_id: TaskId,
    },
}
