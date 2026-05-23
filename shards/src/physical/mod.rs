//! Physical shard module
//!
//! The Physical shard handles supply chain tracking, real estate provenance,
//! and any real-world asset anchoring. It uses an append-only provenance log
//! (a CRDT-friendly structure) to track the history of physical items.

pub mod ops;
pub mod state;
pub mod validator;

pub use ops::PhysicalOp;
pub use state::{PhysicalState, ProvenanceEvent};
pub use validator::PhysicalValidator;
