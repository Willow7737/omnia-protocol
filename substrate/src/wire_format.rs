//! Re-export of wire format types from `omnia-primitives`.
//!
//! This module provides backward compatibility for code that imports
//! wire format types via `use crate::wire_format::...`.

pub use omnia_primitives::wire_format::{
    deserialize_with_version, serialize_with_version, WireFormatError, WIRE_FORMAT_VERSION,
};
