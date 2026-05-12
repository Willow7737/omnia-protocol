//! Identity shard module
//!
//! The Identity shard manages Decentralized Identifiers (DIDs), social
//! recovery configurations, and AI agent identities. It is critical for
//! Phase 0 because it underpins the Self-Sovereign Identity (SSI) system.
//!
//! Layer 4 hardening adds:
//! - Shamir's Secret Sharing for social recovery
//! - Privacy-preserving biometric anchors
//! - AI agent identity with capability-based access control

pub mod agent;
pub mod biometric;
pub mod did;
pub mod ops;
pub mod recovery;
pub mod state;
pub mod validator;

pub use agent::{AgentCapability, AgentIdentity};
pub use biometric::BiometricAnchor;
pub use did::{format_did, validate_did, DidError, DID_METHOD, DID_PREFIX};
pub use ops::{Did, DidUpdate, IdentityOp};
pub use recovery::{RecoveryShare, ShamirRecovery};
pub use state::{DidDocument, IdentityState, RecoveryConfig};
pub use validator::IdentityValidator;
