//! # Omnia Primitives — Shared types for the Omnia Protocol
//!
//! This crate contains the fundamental data types shared across all layers
//! of the Omnia Protocol, with minimal dependencies. It is designed to be
//! depended upon by all other workspace crates without introducing heavy
//! dependency trees (no networking, no async runtime, no database).

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod blake3_domain;
pub mod event;
pub mod state;
pub mod vector_clock;
pub mod wire_format;

// Re-export commonly used types at crate root
pub use blake3_domain::blake3_hash_domain;
pub use event::{
    Event, EventBatch, EventHeader, EventId, EventRequest, EventStatus, EventValidationError, Payload,
    MAX_EVENT_AGE_MS, MAX_PAYLOAD_SIZE, MAX_TIMESTAMP_DRIFT_MS,
};
pub use state::{SerializableState, StateSerializeError};
pub use vector_clock::{CausalOrder, LogicalClock, NodeId, VectorClock, VectorClockError};
pub use wire_format::{deserialize_with_version, serialize_with_version, WireFormatError, WIRE_FORMAT_VERSION};
