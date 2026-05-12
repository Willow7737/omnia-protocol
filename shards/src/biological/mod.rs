//! Biological shard module
//!
//! The Biological shard manages health records, bio-signals, consent
//! registries, and data vaults. It supports zero-knowledge proof queries
//! so that data consumers can verify claims about biological data without
//! accessing the raw data.

pub mod ops;
pub mod state;
pub mod validator;

pub use ops::BiologicalOp;
pub use state::{BiologicalState, ConsentRecord};
pub use validator::BiologicalValidator;
