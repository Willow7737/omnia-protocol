//! Serializable state trait for shard state snapshots
//!
//! Provides a standard interface for serializing and deserializing shard
//! state to/from bytes. All shard state types implement this trait so
//! that the `Shard` trait can take snapshots without knowing the concrete
//! serialization format.

use postcard::{from_bytes, to_allocvec};
use serde::{de::DeserializeOwned, Serialize};

/// Version prefix for state serialization format.
/// This allows future format migrations by incrementing the version byte.
const STATE_FORMAT_VERSION: u8 = 1;

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
    /// The output is prefixed with a version byte to enable future
    /// format migrations.
    fn to_state_bytes(&self) -> Result<Vec<u8>, StateSerializeError> {
        let payload = to_allocvec(self).map_err(|e| StateSerializeError::Serialize(e.to_string()))?;
        let mut bytes = vec![STATE_FORMAT_VERSION];
        bytes.extend(payload);
        Ok(bytes)
    }

    /// Deserialize state from bytes.
    ///
    /// Checks the version prefix byte before deserializing.
    fn from_state_bytes(bytes: &[u8]) -> Result<Self, StateSerializeError> {
        if bytes.is_empty() {
            return Err(StateSerializeError::Deserialize(
                "empty data — missing version byte".to_string(),
            ));
        }
        let version = bytes[0];
        if version != STATE_FORMAT_VERSION {
            return Err(StateSerializeError::Deserialize(format!(
                "unsupported state format version: {version}"
            )));
        }
        from_bytes(&bytes[1..]).map_err(|e| StateSerializeError::Deserialize(e.to_string()))
    }
}

/// Blanket implementation: any `Serialize + DeserializeOwned` type is
/// automatically a `SerializableState`.
impl<T: Serialize + DeserializeOwned> SerializableState for T {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct TestState {
        value: u64,
        label: String,
    }

    #[test]
    fn test_state_roundtrip() {
        let state = TestState {
            value: 42,
            label: "hello".into(),
        };
        let bytes = state.to_state_bytes().unwrap();
        assert_eq!(bytes[0], STATE_FORMAT_VERSION);
        let restored: TestState = TestState::from_state_bytes(&bytes).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn test_empty_bytes_rejected() {
        let result: Result<TestState, _> = TestState::from_state_bytes(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_version_rejected() {
        let state = TestState {
            value: 1,
            label: "x".into(),
        };
        let mut bytes = state.to_state_bytes().unwrap();
        bytes[0] = 99; // Wrong version
        let result: Result<TestState, _> = TestState::from_state_bytes(&bytes);
        assert!(result.is_err());
        match result {
            Err(StateSerializeError::Deserialize(msg)) => {
                assert!(
                    msg.contains("unsupported"),
                    "Error should mention unsupported version: {msg}"
                );
            }
            other => panic!("Expected Deserialize error, got {other:?}"),
        }
    }
}
