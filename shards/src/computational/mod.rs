//! Computational shard module
//!
//! The Computational shard handles AI training tasks, verifiable compute,
//! and proof registries. It manages a task queue where tasks can be
//! submitted, proofs computed, and results verified.

pub mod ops;
pub mod state;
pub mod validator;

pub use ops::ComputationalOp;
pub use state::{ComputationalState, TaskStatus};
pub use validator::ComputationalValidator;
