//! # Omnia Crypto — Cryptographic primitives for the Omnia Protocol
//!
//! This crate provides all cryptographic operations: key generation, signing,
//! verification, BLS aggregation, threshold signatures, VRF leader selection,
//! and encrypted key storage. Heavy dependencies are feature-gated to keep
//! compile times down for consumers that only need basic Ed25519 operations.

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod crypto;
pub mod crypto_schemes;

#[cfg(feature = "keystore")]
pub mod aes_gcm;

#[cfg(feature = "keystore")]
pub mod keystore;

#[cfg(feature = "bls")]
pub mod bls;

#[cfg(feature = "bls")]
pub mod threshold;

pub mod vrf;

// Re-export commonly used types at crate root
pub use crypto::{generate_keypair, NodeKeypair, NodePublicKey, Signature, SignatureError, Signer, Verifier};
pub use crypto_schemes::{CryptoProfile, HashScheme, SchemeVersion, SignatureScheme, VrfScheme, ZkScheme};

#[cfg(feature = "keystore")]
pub use aes_gcm::{aes256gcm_decrypt_aad, aes256gcm_encrypt_aad, generate_nonce, hkdf_aes_key, AesGcmError};

#[cfg(feature = "keystore")]
pub use keystore::{EncryptedKeyStore, KeyPurpose, KeyRotationProof, KeyStoreError, KeyStoreResult};

#[cfg(feature = "bls")]
pub use bls::{
    aggregate_public_keys, aggregate_signatures, verify_aggregate, verify_aggregate_with_pop, BlsError, BlsKeypair,
    BlsProofOfPossession, BlsPublicKey, BlsSignature,
};

#[cfg(feature = "bls")]
pub use threshold::{
    AeadCiphertext, DkgError, DkgPhase, DkgResult, DkgSession, DkgSharePackage, DkgVerificationResult, KeyShare,
    PartialSignature, ThresholdConfig, ThresholdError, ThresholdKeyManager, ThresholdSignature,
};

pub use vrf::{select_leader, vrf_compute, vrf_verify, VrfError, VrfOutput};
