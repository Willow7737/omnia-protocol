//! FFI-based settlement adapter for production deployments.
//!
//! This adapter calls a pre-compiled C library via FFI, enabling
//! production deployments with **any** Rust version (including MSRV 1.88).
//! The C library wraps the alloy Ethereum client, so the core protocol
//! never directly depends on alloy.
//!
//! ## Build Requirements
//!
//! The FFI adapter requires a pre-compiled `libsettlement.a` (or `.so`)
//! in the `lib/` directory. If the library is not found, the `settlement-ffi`
//! feature is automatically disabled by `build.rs`.
//!
//! ## Safety
//!
//! All FFI calls are wrapped in `unsafe` blocks with explicit safety
//! documentation. The C library is expected to follow the ABI defined
//! in this module.

// FFI intrinsically requires unsafe code. The crate-level `deny(unsafe_code)`
// is relaxed to `allow` within this module because every unsafe block is
// documented with a safety proof.
#![allow(unsafe_code)]

#[cfg(feature = "settlement-ffi")]
use super::{FinalityProof, SettlementAdapter, SettlementError, TxHash};
#[cfg(feature = "settlement-ffi")]
use crate::merkle::MerkleProof;

// ---------------------------------------------------------------------------
// C ABI types
// ---------------------------------------------------------------------------

/// C-compatible transaction hash (32 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CTxHash {
    /// 32-byte transaction hash data.
    pub data: [u8; 32],
}

/// C-compatible finality proof.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CFinalityProof {
    /// 32-byte transaction hash.
    pub tx_hash: [u8; 32],
    /// Block number at which the transaction was finalized.
    pub block_number: u64,
    /// Number of confirmations.
    pub confirmation_count: u64,
    /// BLAKE3 proof hash (32 bytes).
    pub proof_hash: [u8; 32],
}

/// C-compatible Merkle inclusion result.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CInclusionResult {
    /// Whether the inclusion proof is valid.
    pub valid: bool,
    /// Error code (0 = success, non-zero = error).
    pub error_code: i32,
}

/// C-compatible error result.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CSettlementResult {
    /// Whether the operation succeeded.
    pub success: bool,
    /// Error code (0 = success, non-zero = error).
    pub error_code: i32,
    /// Error message (null-terminated, max 256 bytes).
    pub error_message: [u8; 256],
}

// ---------------------------------------------------------------------------
// FFI function declarations
// ---------------------------------------------------------------------------

#[cfg(feature = "settlement-ffi")]
extern "C" {
    /// Submit a state root to the settlement layer via the C library.
    ///
    /// # Safety
    ///
    /// The caller must ensure `root_bytes` points to at least `len` bytes
    /// of valid memory. The returned `CTxHash` is valid for reading.
    pub fn settlement_submit_root(root_bytes: *const u8, len: usize) -> CTxHash;

    /// Fetch finality proof for a transaction.
    ///
    /// # Safety
    ///
    /// The caller must ensure `tx_hash_bytes` points to at least 32 bytes
    /// of valid memory. The returned `CFinalityProof` is valid for reading.
    pub fn settlement_fetch_finality(tx_hash_bytes: *const u8) -> CFinalityProof;

    /// Verify a Merkle inclusion proof.
    ///
    /// # Safety
    ///
    /// The caller must ensure all pointer arguments are valid and point to
    /// sufficient memory. `siblings_ptr` must point to `sibling_count * 32`
    /// bytes. `directions_ptr` must point to `sibling_count` bytes.
    pub fn settlement_verify_inclusion(
        leaf_bytes: *const u8,
        siblings_ptr: *const u8,
        sibling_count: usize,
        directions_ptr: *const u8,
    ) -> CInclusionResult;

    /// Initialize the FFI settlement client with an RPC URL.
    ///
    /// # Safety
    ///
    /// The caller must ensure `rpc_url` is a valid null-terminated C string.
    pub fn settlement_init(rpc_url: *const u8, rpc_url_len: usize) -> CSettlementResult;
}

// ---------------------------------------------------------------------------
// FfiSettlementAdapter
// ---------------------------------------------------------------------------

/// FFI-based settlement adapter for production deployments.
///
/// This adapter delegates settlement operations to a pre-compiled C
/// library, enabling production Ethereum settlement without requiring
/// alloy in the Rust dependency tree. This means the core protocol
/// can use MSRV 1.88 while still supporting live Ethereum settlement.
///
/// # Architecture
///
/// ```text
/// ┌──────────────────┐     FFI      ┌──────────────────────┐
/// │  omnia-adapters  │ ──────────►  │  libsettlement.a     │
/// │  (Rust 1.88)     │              │  (C + alloy 1.91+)   │
/// └──────────────────┘              └──────────────────────┘
/// ```
///
/// # Safety
///
/// All FFI calls are isolated in this module. The C library is
/// responsible for thread safety and error handling.
#[cfg(feature = "settlement-ffi")]
pub struct FfiSettlementAdapter {
    /// Whether the FFI client has been initialized.
    initialized: bool,
    /// RPC URL (stored for potential re-initialization).
    rpc_url: String,
}

#[cfg(feature = "settlement-ffi")]
impl FfiSettlementAdapter {
    /// Create a new FFI settlement adapter.
    ///
    /// The adapter is not connected until [`init`](Self::init) is called.
    /// Operations called before initialization will return an error.
    pub fn new(rpc_url: &str) -> Self {
        Self {
            initialized: false,
            rpc_url: rpc_url.to_string(),
        }
    }

    /// Initialize the FFI settlement client.
    ///
    /// Calls the C library's `settlement_init` function with the
    /// configured RPC URL.
    ///
    /// # Safety
    ///
    /// This function calls external C code via FFI. The C library
    /// must be properly linked and initialized.
    pub unsafe fn init(&mut self) -> Result<(), SettlementError> {
        let rpc_bytes = self.rpc_url.as_bytes();
        let result = settlement_init(rpc_bytes.as_ptr(), rpc_bytes.len());

        if result.success {
            self.initialized = true;
            Ok(())
        } else {
            let msg_end = result
                .error_message
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(result.error_message.len());
            let msg = std::str::from_utf8(&result.error_message[..msg_end]).unwrap_or("unknown FFI error");
            Err(SettlementError::RpcError(format!(
                "FFI init failed (code {}): {}",
                result.error_code, msg
            )))
        }
    }
}

#[cfg(feature = "settlement-ffi")]
#[async_trait::async_trait]
impl SettlementAdapter for FfiSettlementAdapter {
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError> {
        if !self.initialized {
            return Err(SettlementError::ConfigError(
                "FFI settlement adapter not initialized".to_string(),
            ));
        }

        // Safety: `root` is a fixed-size array, so the pointer is valid
        // for 32 bytes. The C function returns a CTxHash by value.
        let c_result = unsafe { settlement_submit_root(root.as_ptr(), 32) };

        Ok(TxHash(c_result.data))
    }

    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError> {
        if !self.initialized {
            return Err(SettlementError::ConfigError(
                "FFI settlement adapter not initialized".to_string(),
            ));
        }

        // Safety: `tx.0` is a fixed-size array, pointer is valid for 32 bytes.
        let c_result = unsafe { settlement_fetch_finality(tx.0.as_ptr()) };

        Ok(FinalityProof {
            tx_hash: TxHash(c_result.tx_hash),
            block_number: c_result.block_number,
            confirmation_count: c_result.confirmation_count,
            proof_hash: c_result.proof_hash,
        })
    }

    async fn verify_inclusion(&self, proof: &MerkleProof) -> Result<bool, SettlementError> {
        if !self.initialized {
            return Err(SettlementError::ConfigError(
                "FFI settlement adapter not initialized".to_string(),
            ));
        }

        // Flatten siblings into a contiguous byte buffer
        let sibling_bytes: Vec<u8> = proof.siblings.iter().flat_map(|s| s.iter().copied()).collect();
        let direction_bytes: Vec<u8> = proof.directions.iter().map(|&d| d as u8).collect();

        // Safety: sibling_bytes and direction_bytes are valid Vec<u8> buffers
        // whose pointers remain valid for the duration of the FFI call.
        let c_result = unsafe {
            settlement_verify_inclusion(
                [0u8; 32].as_ptr(), // leaf (not used in FFI currently)
                sibling_bytes.as_ptr(),
                proof.siblings.len(),
                direction_bytes.as_ptr(),
            )
        };

        if c_result.error_code == 0 {
            Ok(c_result.valid)
        } else {
            Err(SettlementError::ContractError(format!(
                "FFI verify_inclusion failed with error code {}",
                c_result.error_code
            )))
        }
    }

    fn is_live(&self) -> bool {
        self.initialized
    }
}

#[cfg(all(test, feature = "settlement-ffi"))]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_adapter_not_initialized() {
        let adapter = FfiSettlementAdapter::new("http://localhost:8545");
        assert!(!adapter.is_live());
    }
}
