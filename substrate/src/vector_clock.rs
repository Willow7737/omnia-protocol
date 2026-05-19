//! Re-export of vector clock types from `omnia-primitives`.
//!
//! This module provides backward compatibility for code that imports
//! vector clock types via `use crate::vector_clock::...`.

pub use omnia_primitives::vector_clock::{
    CausalOrder, LogicalClock, NodeId, VectorClock, VectorClockError,
};
