//! Wallet challenge/signature authentication
//!
//! Self-custodial clients (e.g. the Omnia mobile wallet) hold an Ed25519
//! keypair locally and must obtain a node JWT **without** ever sharing a
//! secret with the node. This module implements a two-step
//! challenge/response login:
//!
//! - `POST /api/v1/auth/challenge` — the client presents its Ed25519 public
//!   key; the node returns a fresh single-use `nonce` (and the DID derived
//!   from that key). The nonce is retained in memory with a short TTL.
//! - `POST /api/v1/auth/login` — the client signs `"omnia-auth:" + nonce`
//!   with its private key and returns the signature. The node verifies the
//!   signature against the public key, consumes the nonce, registers the DID
//!   in the economics quota system if it is new, and issues a JWT via
//!   [`crate::api::auth::create_token`].
//!
//! Both endpoints are **public** (no JWT required) — they are the entry point
//! that produces a JWT. They are still subject to the global rate limiter.
//!
//! # DID derivation
//!
//! The DID is derived deterministically from the public key so any client can
//! compute it offline: `did:omnia:` followed by the first 32 hex characters of
//! `sha256(public_key_bytes)`. SHA-256 (rather than BLAKE3) is used here so the
//! mobile wallet can reproduce the exact rule with a ubiquitous, well-tested
//! hash implementation, and display the user's DID before ever contacting the
//! node.
//!
//! # Security properties
//!
//! - **Replay protection** — each nonce is single-use and expires after
//!   [`CHALLENGE_TTL_MS`]; a captured signature cannot be reused.
//! - **Domain separation** — the signed message is prefixed with
//!   [`AUTH_MESSAGE_PREFIX`] so a signature minted here can never be mistaken
//!   for a signature over some other protocol message.
//! - **Strict verification** — `verify_strict` rejects small-order / malleable
//!   public keys.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use utoipa::ToSchema;

use omnia_substrate::crypto::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::api::auth::create_token;
use crate::state::AppState;

/// Lifetime of an issued challenge nonce, in milliseconds (5 minutes).
pub const CHALLENGE_TTL_MS: u64 = 5 * 60 * 1000;

/// Lifetime of the JWT issued on successful login, in seconds (24 hours).
pub const TOKEN_TTL_SECS: u64 = 86_400;

/// Domain-separation prefix prepended to the nonce before signing.
///
/// The client must sign the byte string `"omnia-auth:" + nonce_hex`.
pub const AUTH_MESSAGE_PREFIX: &str = "omnia-auth:";

/// Length of the Ed25519 public key, in bytes.
const PUBKEY_LEN: usize = 32;

/// Length of an Ed25519 signature, in bytes.
const SIGNATURE_LEN: usize = 64;

/// A pending challenge: the public key it was issued to and its expiry.
#[derive(Debug, Clone)]
pub struct ChallengeEntry {
    /// Hex-encoded public key the challenge was issued to.
    pub public_key: String,
    /// Unix-millisecond timestamp after which the nonce is invalid.
    pub expires_at_ms: u64,
}

/// In-memory store of outstanding challenges, keyed by nonce (hex).
///
/// Wrapped in an `Arc<Mutex<..>>` and cloned into [`AppState`], mirroring the
/// interior-mutability pattern used by the other shared node state.
pub type ChallengeStore = Arc<Mutex<HashMap<String, ChallengeEntry>>>;

/// Construct an empty challenge store.
pub fn new_challenge_store() -> ChallengeStore {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Current Unix time in milliseconds.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Derive the deterministic DID for an Ed25519 public key.
///
/// Returns `did:omnia:` + the first 32 hex characters of
/// `sha256(public_key_bytes)`. This rule is duplicated verbatim in the mobile
/// wallet so both sides agree on the identity for a given key.
pub fn did_from_public_key(public_key_bytes: &[u8]) -> String {
    let digest = Sha256::digest(public_key_bytes);
    let hex_digest = hex::encode(digest);
    format!("did:omnia:{}", &hex_digest[..32])
}

/// Decode a hex string into a fixed-size byte array of length `N`.
fn decode_hex_fixed<const N: usize>(hex_str: &str) -> Result<[u8; N], String> {
    let bytes = hex::decode(hex_str.trim()).map_err(|e| format!("invalid hex: {e}"))?;
    <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| format!("expected {N} bytes, got {} ", hex_str.len() / 2))
}

/// Build a `(StatusCode, Json)` error tuple.
fn err(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message.into() })))
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Request body for `POST /api/v1/auth/challenge`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ChallengeRequest {
    /// Hex-encoded 32-byte Ed25519 public key.
    pub public_key: String,
}

/// Response body for `POST /api/v1/auth/challenge`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ChallengeResponse {
    /// DID derived from the public key (`did:omnia:...`).
    pub did: String,
    /// Hex-encoded single-use nonce the client must sign.
    pub nonce: String,
    /// Unix-millisecond timestamp after which the nonce expires.
    pub expires_at: u64,
    /// The exact message string the client must sign
    /// (`"omnia-auth:" + nonce`), provided for convenience.
    pub message: String,
}

/// Request body for `POST /api/v1/auth/login`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct LoginRequest {
    /// Hex-encoded 32-byte Ed25519 public key (must match the challenge).
    pub public_key: String,
    /// Hex-encoded 64-byte Ed25519 signature over `"omnia-auth:" + nonce`.
    pub signature: String,
    /// The nonce that was issued by `/auth/challenge`.
    pub nonce: String,
}

/// Response body for `POST /api/v1/auth/login`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoginResponse {
    /// DID the JWT was issued for.
    pub did: String,
    /// Signed JWT to use as `Authorization: Bearer <token>`.
    pub token: String,
    /// Token lifetime in seconds.
    pub expires_in: u64,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Handler for `POST /api/v1/auth/challenge`.
///
/// Validates the public key, prunes expired nonces, generates a fresh
/// single-use nonce bound to the key, and returns it alongside the derived
/// DID and the exact message to sign.
#[utoipa::path(
    post,
    path = "/api/v1/auth/challenge",
    request_body = ChallengeRequest,
    responses(
        (status = 200, description = "Challenge issued", body = ChallengeResponse),
        (status = 400, description = "Invalid public key"),
    )
)]
pub async fn request_challenge(
    State(state): State<AppState>,
    Json(body): Json<ChallengeRequest>,
) -> Result<(StatusCode, Json<ChallengeResponse>), (StatusCode, Json<Value>)> {
    // Validate the public key is well-formed Ed25519.
    let pubkey_bytes = decode_hex_fixed::<PUBKEY_LEN>(&body.public_key)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid public_key: {e}")))?;
    VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid public_key: {e}")))?;

    // Normalize the stored key to lower-case hex so lookups are stable.
    let public_key = hex::encode(pubkey_bytes);

    // Generate a 32-byte random nonce.
    let mut nonce_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = hex::encode(nonce_bytes);

    let now = now_ms();
    let expires_at = now + CHALLENGE_TTL_MS;

    {
        let mut store = state.challenges.lock().await;
        // Lazily prune expired nonces to keep the map bounded.
        store.retain(|_, entry| entry.expires_at_ms > now);
        store.insert(
            nonce.clone(),
            ChallengeEntry {
                public_key: public_key.clone(),
                expires_at_ms: expires_at,
            },
        );
    }

    let did = did_from_public_key(&pubkey_bytes);
    let message = format!("{AUTH_MESSAGE_PREFIX}{nonce}");

    Ok((
        StatusCode::OK,
        Json(ChallengeResponse {
            did,
            nonce,
            expires_at,
            message,
        }),
    ))
}

/// Handler for `POST /api/v1/auth/login`.
///
/// Verifies the signature over the challenge message, consumes the nonce,
/// registers the DID if new, and issues a JWT.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login succeeded", body = LoginResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Signature verification failed"),
        (status = 500, description = "JWT secret not configured"),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<(StatusCode, Json<LoginResponse>), (StatusCode, Json<Value>)> {
    // Decode and validate inputs.
    let pubkey_bytes = decode_hex_fixed::<PUBKEY_LEN>(&body.public_key)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid public_key: {e}")))?;
    let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid public_key: {e}")))?;
    let sig_bytes = decode_hex_fixed::<SIGNATURE_LEN>(&body.signature)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("invalid signature: {e}")))?;
    let signature = Signature::from_bytes(&sig_bytes);

    let public_key = hex::encode(pubkey_bytes);
    let now = now_ms();

    // Look up and consume the nonce (single-use). Holding the lock across the
    // remove ensures two concurrent logins cannot both consume it.
    let entry = {
        let mut store = state.challenges.lock().await;
        match store.remove(&body.nonce) {
            Some(e) => e,
            None => {
                return Err(err(
                    StatusCode::UNAUTHORIZED,
                    "unknown or already-used nonce — request a new challenge",
                ))
            }
        }
    };

    // The nonce must belong to this public key and still be valid.
    if entry.expires_at_ms <= now {
        return Err(err(StatusCode::UNAUTHORIZED, "challenge expired — request a new one"));
    }
    if entry.public_key != public_key {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "nonce was issued to a different public key",
        ));
    }

    // Verify the signature over the domain-separated challenge message.
    let message = format!("{AUTH_MESSAGE_PREFIX}{}", body.nonce);
    verifying_key
        .verify_strict(message.as_bytes(), &signature)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "signature verification failed"))?;

    // Signature is valid — derive the DID and ensure it is registered so the
    // wallet has a UBC quota/balance to display and spend.
    let did = did_from_public_key(&pubkey_bytes);
    {
        let mut economics = state.economics.lock().await;
        if !economics.quota.is_registered(&did) {
            economics.quota.register_did(&did);
            tracing::info!(did = %did, "Registered new wallet DID via challenge/signature login");
        }
    }

    // Issue the JWT.
    let token = create_token(&did, TOKEN_TTL_SECS)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("failed to mint token: {e}")))?;

    tracing::info!(did = %did, "Wallet login succeeded");

    Ok((
        StatusCode::OK,
        Json(LoginResponse {
            did,
            token,
            expires_in: TOKEN_TTL_SECS,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_substrate::crypto::{Signer, SigningKey};

    fn test_keypair() -> SigningKey {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        SigningKey::from_bytes(&seed)
    }

    #[test]
    fn did_derivation_is_deterministic_and_prefixed() {
        let pk = [7u8; 32];
        let did1 = did_from_public_key(&pk);
        let did2 = did_from_public_key(&pk);
        assert_eq!(did1, did2);
        assert!(did1.starts_with("did:omnia:"));
        // 10 chars for "did:omnia:" + 32 hex chars.
        assert_eq!(did1.len(), "did:omnia:".len() + 32);
    }

    #[test]
    fn did_derivation_matches_shared_cross_repo_vector() {
        // This exact literal is also asserted in the Flutter wallet's
        // key_manager_test.dart. If either side changes the hash or the
        // truncation, this vector breaks and the wallet <-> node identities
        // silently diverge. Keep the two in lockstep.
        let did = did_from_public_key(&[7u8; 32]);
        assert_eq!(did, "did:omnia:4bb06f8e4e3a7715d201d573d0aa4237");
    }

    #[test]
    fn did_derivation_differs_per_key() {
        assert_ne!(did_from_public_key(&[1u8; 32]), did_from_public_key(&[2u8; 32]));
    }

    #[test]
    fn signature_over_challenge_verifies() {
        let sk = test_keypair();
        let vk = sk.verifying_key();
        let nonce = "deadbeef";
        let message = format!("{AUTH_MESSAGE_PREFIX}{nonce}");
        let sig = sk.sign(message.as_bytes());
        assert!(vk.verify_strict(message.as_bytes(), &sig).is_ok());
        // Wrong message must fail.
        assert!(vk.verify_strict(b"omnia-auth:cafe", &sig).is_err());
    }

    #[test]
    fn decode_hex_fixed_enforces_length() {
        assert!(decode_hex_fixed::<32>(&"ab".repeat(32)).is_ok());
        assert!(decode_hex_fixed::<32>("abcd").is_err());
        assert!(decode_hex_fixed::<32>("nothex").is_err());
    }
}
