//! DID method specification for the `did:omnia` method
//!
//! Defines the format and validation rules for Omnia protocol DIDs.
//! The method follows the W3C DID Core specification with the format:
//!
//! ```text
//! did:omnia:<method-specific-identifier>
//! ```
//!
//! where `<method-specific-identifier>` is a hex-encoded Ed25519 public key
//! (64 hex characters representing 32 bytes).

use serde::{Deserialize, Serialize};

/// The DID method name.
pub const DID_METHOD: &str = "omnia";

/// The DID method prefix.
pub const DID_PREFIX: &str = "did:omnia:";

/// Validate a DID string according to the `did:omnia` method spec.
///
/// A valid `did:omnia` DID must:
/// - Start with `did:omnia:`
/// - Have a method-specific identifier that is a valid hex-encoded 32-byte
///   Ed25519 public key (64 hex characters)
pub fn validate_did(did: &str) -> Result<(), DidError> {
    if !did.starts_with(DID_PREFIX) {
        return Err(DidError::InvalidPrefix(did.to_string()));
    }

    let identifier = &did[DID_PREFIX.len()..];

    if identifier.len() != 64 {
        return Err(DidError::InvalidIdentifierLength {
            expected: 64,
            got: identifier.len(),
        });
    }

    // Verify that the identifier is valid hex
    hex::decode(identifier).map_err(|_| DidError::InvalidHex(identifier.to_string()))?;

    Ok(())
}

/// Construct a `did:omnia` DID from a 32-byte public key.
pub fn format_did(public_key: &[u8; 32]) -> String {
    format!("{DID_PREFIX}{}", hex::encode(public_key))
}

/// Errors that can occur during DID validation.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq, Serialize, Deserialize)]
pub enum DidError {
    /// The DID does not start with the `did:omnia:` prefix.
    #[error("DID does not start with 'did:omnia:': {0}")]
    InvalidPrefix(String),

    /// The method-specific identifier has the wrong length.
    #[error("DID identifier has wrong length: expected {expected} hex chars, got {got}")]
    InvalidIdentifierLength {
        /// Expected length in hex characters.
        expected: usize,
        /// Actual length.
        got: usize,
    },

    /// The method-specific identifier contains non-hex characters.
    #[error("DID identifier is not valid hex: {0}")]
    InvalidHex(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_did() {
        let pubkey = [0xABu8; 32];
        let did = format_did(&pubkey);
        assert!(validate_did(&did).is_ok());
    }

    #[test]
    fn test_invalid_prefix() {
        assert!(validate_did("did:other:abcdef1234").is_err());
        assert!(validate_did("omnia:abcdef1234").is_err());
    }

    #[test]
    fn test_invalid_identifier_length() {
        // Too short
        let short = format!("{DID_PREFIX}abcdef");
        assert!(validate_did(&short).is_err());

        // Too long
        let long = format!("{DID_PREFIX}{}", "abcdef1234".repeat(10));
        assert!(validate_did(&long).is_err());
    }

    #[test]
    fn test_invalid_hex() {
        let invalid = format!(
            "{DID_PREFIX}ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
        );
        assert!(validate_did(&invalid).is_err());
    }

    #[test]
    fn test_format_did() {
        let pubkey = [0x01u8; 32];
        let did = format_did(&pubkey);
        assert_eq!(did, format!("{DID_PREFIX}{}", "01".repeat(32)));
    }
}
