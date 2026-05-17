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

/// Deserialize a value, checking wire-format version.
///
/// Expects the first byte to be `WIRE_FORMAT_VERSION`. Returns an error
/// if the version byte is missing or doesn't match.
pub fn deserialize_with_version<'de, T: serde::Deserialize<'de>>(
    bytes: &'de [u8],
) -> Result<T, postcard::Error> {
    if bytes.is_empty() {
        return Err(postcard::Error::DeserializeUnexpectedEnd);
    }
    if bytes[0] != WIRE_FORMAT_VERSION {
        return Err(postcard::Error::DeserializeUnexpectedEnd);
    }
    postcard::from_bytes(&bytes[1..])
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
        bytes[0] = 0xFF; // Wrong version
        let result: Result<TestMsg, _> = deserialize_with_version(&bytes);
        assert!(result.is_err());
    }
}
