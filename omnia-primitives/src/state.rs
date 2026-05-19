//! Serializable state trait for shard state snapshots
//!
//! Provides a standard interface for serializing and deserializing shard
//! state to/from bytes. All shard state types implement this trait so
//! that the `Shard` trait can take snapshots without knowing the concrete
//! serialization format.

use postcard::{from_bytes, to_allocvec};
use serde::{de::DeserializeOwned, Serialize};

/// Error type for state serialization operations.
#[derive(Debug, thiserror::Error)]
pub enum StateSerializeError {
    /// Serialization failed.
    #[error("Serialization failed: {0}")]
    Serialize(String),
    /// Deserialization failed.
    #[error("Deserialization failed: {0}")]
    Deserialize(String),
}

/// Trait for types that can be serialized to and deserialized from bytes
/// using the postcard format.
///
/// All shard state types implement this trait so that the `Shard` trait's
/// `state_snapshot()` method has a uniform serialization interface.
///
/// # Implementation
///
/// A blanket implementation is provided for any type that implements
/// `Serialize + DeserializeOwned`. No manual implementation is needed
/// for standard shard state types.
pub trait SerializableState: Serialize + DeserializeOwned + Sized {
    /// Serialize this state to bytes.
    ///
    /// Uses `postcard` for compact deterministic binary encoding.
    fn to_state_bytes(&self) -> Result<Vec<u8>, StateSerializeError> {
        to_allocvec(self).map_err(|e| StateSerializeError::Serialize(e.to_string()))
    }

    /// Deserialize state from bytes.
    fn from_state_bytes(bytes: &[u8]) -> Result<Self, StateSerializeError> {
        from_bytes(bytes).map_err(|e| StateSerializeError::Deserialize(e.to_string()))
    }
}

/// Blanket implementation: any `Serialize + DeserializeOwned` type is
/// automatically a `SerializableState`.
impl<T: Serialize + DeserializeOwned> SerializableState for T {}
