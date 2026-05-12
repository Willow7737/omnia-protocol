//! Identity shard module
//!
//! The Identity shard manages Decentralized Identifiers (DIDs), social
//! recovery configurations, and AI agent identities. It is critical for
//! Phase 0 because it underpins the Self-Sovereign Identity (SSI) system.

pub mod ops;
pub mod state;
pub mod validator;

pub use ops::{AgentIdentity, Did, DidUpdate, IdentityOp, RecoveryShare};
pub use state::{DidDocument, IdentityState, RecoveryConfig};
pub use validator::IdentityValidator;
