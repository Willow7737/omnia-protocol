//! Wire-format versioning for serialized protocol messages.
//!
//! All serialized messages on the wire are prefixed with a version byte
//! to enable future format migrations without breaking backward compatibility.
//! The current format uses `postcard` (a `no_std`-compatible, deterministic
//! serde serializer) which replaces the unmaintained `bincode 1.x`
//! (RUSTSEC-2025-0141).

/// Current wire format version for serialized protocol messages.
///
/// - Version `0` was the old bincode 1.x format (no version prefix).
/// - Version `1` is the current postcard format (with version prefix).
pub const WIRE_FORMAT_VERSION: u8 = 1;

/// Maximum allowed input size for postcard deserialization (10 MiB).
///
/// This prevents denial-of-service attacks via excessively large payloads
/// that could exhaust memory during deserialization.
pub const MAX_POSTCARD_INPUT_SIZE: usize = 10 * 1024 * 1024; // 10 MiB

/// Errors that can occur during wire-format deserialization.
#[derive(Debug, thiserror::Error)]
pub enum WireFormatError {
    /// The input data is empty (missing version byte).
    #[error("Empty data — missing version byte")]
    EmptyData,
    /// The version byte is not recognized.
    #[error("Unknown wire format version: {0}")]
    UnknownVersion(u8),
    /// Deserialization failed.
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),
}

/// Serialize a value with wire-format version prefix.
///
/// The output is `[WIRE_FORMAT_VERSION] ++ postcard(value)`.
/// This allows future format migrations by incrementing the version byte
/// and changing the serialization logic.
pub fn serialize_with_version<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, postcard::Error> {
    let mut bytes = vec![WIRE_FORMAT_VERSION];
    bytes.extend(postcard::to_allocvec(value)?);
    Ok(bytes)
}

/// Deserialize a value, handling wire-format version.
///
/// Supports two format versions:
/// - Version `0`: Legacy bincode 1.x format (for backward compatibility)
/// - Version `1`: Current postcard format
///
/// Returns an error for empty data, unknown version bytes, or inputs
/// exceeding the size limit for the respective format.
pub fn deserialize_with_version<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, WireFormatError> {
    if bytes.is_empty() {
        return Err(WireFormatError::EmptyData);
    }
    let version = bytes[0];
    match version {
        0 => {
            // Legacy bincode format - limit input size for safety
            #[cfg(feature = "legacy-wire")]
            {
                const MAX_BINCODE_INPUT_SIZE: usize = 10 * 1024 * 1024; // 10 MiB
                if bytes.len() > MAX_BINCODE_INPUT_SIZE {
                    return Err(WireFormatError::DeserializationFailed(
                        "legacy bincode input exceeds size limit".to_string(),
                    ));
                }
                bincode::deserialize(&bytes[1..]).map_err(|e| WireFormatError::DeserializationFailed(e.to_string()))
            }
            #[cfg(not(feature = "legacy-wire"))]
            {
                let _ = bytes;
                Err(WireFormatError::DeserializationFailed(
                    "legacy bincode format not supported (enable 'legacy-wire' feature)".to_string(),
                ))
            }
        }
        1 => {
            // Current postcard format — enforce size limit to prevent DoS
            if bytes.len() > MAX_POSTCARD_INPUT_SIZE {
                return Err(WireFormatError::DeserializationFailed(
                    "postcard input exceeds size limit".to_string(),
                ));
            }
            postcard::from_bytes(&bytes[1..]).map_err(|e| WireFormatError::DeserializationFailed(e.to_string()))
        }
        v => Err(WireFormatError::UnknownVersion(v)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct TestMsg {
        id: u64,
        label: String,
    }

    #[test]
    fn test_roundtrip_with_version() {
        let msg = TestMsg {
            id: 42,
            label: "hello".into(),
        };
        let bytes = serialize_with_version(&msg).unwrap();
        assert_eq!(bytes[0], WIRE_FORMAT_VERSION);
        let restored: TestMsg = deserialize_with_version(&bytes).unwrap();
        assert_eq!(restored, msg);
    }

    #[test]
    fn test_empty_bytes_rejected() {
        let result: Result<TestMsg, _> = deserialize_with_version(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_version_rejected() {
        let msg = TestMsg {
            id: 1,
            label: "x".into(),
        };
        let mut bytes = serialize_with_version(&msg).unwrap();
        bytes[0] = 0xFF; // Unknown version
        let result: Result<TestMsg, _> = deserialize_with_version(&bytes);
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "legacy-wire")]
    fn test_legacy_bincode_deserialization() {
        // Simulate a v0 (bincode) serialized message
        let msg = TestMsg {
            id: 99,
            label: "legacy".into(),
        };
        let bincode_bytes = bincode::serialize(&msg).unwrap();
        let mut bytes = vec![0u8]; // version 0
        bytes.extend(bincode_bytes);

        let restored: TestMsg = deserialize_with_version(&bytes).unwrap();
        assert_eq!(restored, msg);
    }

    #[test]
    fn test_unknown_version_returns_error() {
        let bytes = [255u8, 0, 0, 0];
        let result: Result<TestMsg, _> = deserialize_with_version(&bytes);
        assert!(matches!(result, Err(WireFormatError::UnknownVersion(255))));
    }

    #[test]
    fn test_postcard_size_limit_enforced() {
        // Create bytes that claim to be version 1 but exceed the size limit
        let mut bytes = vec![1u8]; // version 1
        bytes.extend(vec![0u8; MAX_POSTCARD_INPUT_SIZE]); // exceeds limit
        let result: Result<TestMsg, _> = deserialize_with_version(&bytes);
        assert!(result.is_err(), "Should reject input exceeding MAX_POSTCARD_INPUT_SIZE");
        match result {
            Err(WireFormatError::DeserializationFailed(msg)) => {
                assert!(msg.contains("size limit"), "Error should mention size limit: {msg}");
            }
            other => panic!("Expected DeserializationFailed, got {other:?}"),
        }
    }
}
