//! Re-export of event types from `omnia-primitives`.
//!
//! This module provides backward compatibility for code that imports
//! event types via `use crate::event::...`.

pub use omnia_primitives::event::{
    Event, EventBatch, EventHeader, EventId, EventRequest, EventStatus, EventValidationError,
    MAX_EVENT_AGE_MS, MAX_PAYLOAD_SIZE, MAX_TIMESTAMP_DRIFT_MS, Payload,
};
