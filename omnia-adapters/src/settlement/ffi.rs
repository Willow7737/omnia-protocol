//! FFI-based settlement adapter for production deployments.
//!
//! This adapter calls a pre-compiled C library via FFI, enabling
//! production deployments with **any** Rust version (including MSRV 1.88).
//! The C library wraps the alloy Ethereum client, so the core protocol
//! never directly depends on alloy.
//!
//! ## Build Requirements
//!
//! The FFI adapter requires a pre-compiled `libsettlement.a` (Linux/macOS)
//! or `settlement.lib` (Windows) in the `lib/` directory. If the library
//! is not found, the `has_settlement_lib` cfg is not emitted by `build.rs`,
//! and the FFI linkage code is excluded — even if the `settlement-ffi`
//! Cargo feature is enabled (e.g., via `--all-features`).
//!
//! ## Two-Gate Design
//!
//! The `settlement-ffi` Cargo feature and the `has_settlement_lib` build
//! script cfg work together:
//!
//! - `settlement-ffi` alone: C ABI types compile, but no FFI linkage.
//!   This allows `--all-features` to work without linker errors on
//!   systems that lack the pre-compiled C library.
//! - `settlement-ffi` + `has_settlement_lib`: Full FFI adapter with
//!   extern declarations, struct, and trait impl. Requires the static
//!   library to be present at link time.
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

#[cfg(all(feature = "settlement-ffi", has_settlement_lib))]
use super::{FinalityProof, SettlementAdapter, SettlementError, TxHash};
#[cfg(all(feature = "settlement-ffi", has_settlement_lib))]
use crate::merkle::MerkleProof;

// ---------------------------------------------------------------------------
// C ABI types
// ---------------------------------------------------------------------------
//
// These types are always compiled when the `settlement-ffi` feature is on,
// even without the pre-compiled library. This allows downstream code to
// reference the C ABI types for type-checking and documentation without
// requiring the actual FFI linkage.

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
//
// These extern "C" declarations reference symbols provided by the
// pre-compiled C library. They are only compiled when the library
// is actually present (has_settlement_lib), because otherwise the
// linker would emit "unresolved external symbol" errors.

#[cfg(all(feature = "settlement-ffi", has_settlement_lib))]
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
    /// The caller must ensure `rpc_url` points to at least `rpc_url_len`
    /// bytes of valid memory. The C side reads exactly `rpc_url_len` bytes
    /// (the buffer is NOT required to be null-terminated; see the P0-8 fix
    /// in `FfiSettlementAdapter::init` for why we still append a `\0`
    /// byte defensively).
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
///
/// # Concurrency
///
/// `FfiSettlementAdapter` is `Send + Sync` because all shared mutable
/// state is protected by atomic operations. The `initialized` flag is
/// an `AtomicBool` (P0-8 fix: previously a plain `bool`, which is
/// unsound under concurrent access from the async trait methods that
/// implement `SettlementAdapter: Send + Sync`).
#[cfg(all(feature = "settlement-ffi", has_settlement_lib))]
pub struct FfiSettlementAdapter {
    /// Whether the FFI client has been initialized.
    ///
    /// P0-8 fix: this is an `AtomicBool` so that concurrent `submit_root`/
    /// `fetch_finality`/`verify_inclusion`/`is_live` calls from multiple
    /// threads do not race on a non-atomic `bool` read/write (which is
    /// undefined behavior in Rust and would let a peer thread observe a
    /// torn value or skip the initialization gate entirely).
    initialized: std::sync::atomic::AtomicBool,
    /// RPC URL (stored for potential re-initialization).
    rpc_url: String,
}

#[cfg(all(feature = "settlement-ffi", has_settlement_lib))]
impl FfiSettlementAdapter {
    /// Create a new FFI settlement adapter.
    ///
    /// The adapter is not connected until `init` is called.
    /// Operations called before initialization will return an error.
    pub fn new(rpc_url: &str) -> Self {
        Self {
            initialized: std::sync::atomic::AtomicBool::new(false),
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
        // P0-8 fix: the C `settlement_init` is documented as taking
        // (ptr, len), but defensively append a `\0` so that if the C
        // implementation ALSO happens to treat the buffer as a
        // null-terminated C string (a common implementation mistake),
        // we do not read past the end of `rpc_bytes` into unrelated
        // heap memory. The trailing `\0` is excluded from the length
        // we pass so a (ptr, len)-style C function still gets the exact
        // RPC URL with no terminator.
        let mut rpc_bytes = self.rpc_url.as_bytes().to_vec();
        rpc_bytes.push(0);
        let result = settlement_init(rpc_bytes.as_ptr(), rpc_bytes.len().saturating_sub(1));

        if result.success {
            // P0-8 fix: store through an AtomicBool with SeqCst ordering
            // so that subsequent `submit_root` / `fetch_finality` /
            // `verify_inclusion` calls on other threads observe the
            // initialization before any FFI call is made.
            self.initialized.store(true, std::sync::atomic::Ordering::SeqCst);
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

#[cfg(all(feature = "settlement-ffi", has_settlement_lib))]
#[async_trait::async_trait]
impl SettlementAdapter for FfiSettlementAdapter {
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError> {
        // P0-8 fix: load the initialization flag atomically. Previously
        // this was a plain `bool` read, which is UB under concurrent
        // access from multiple async tasks sharing the same adapter.
        if !self.initialized.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(SettlementError::ConfigError(
                "FFI settlement adapter not initialized".to_string(),
            ));
        }

        // SAFETY: `root` is a fixed-size `[u8; 32]` array allocated on
        // the stack of this function; `root.as_ptr()` is therefore valid
        // for reads of exactly 32 bytes for the duration of the FFI call.
        // The C function returns a `CTxHash` by value (a 32-byte POD), so
        // there is no aliasing or lifetime concern on the return path.
        // The C library is responsible for not storing the pointer beyond
        // the call (per the documented ABI in the `extern "C"` block).
        let c_result = unsafe { settlement_submit_root(root.as_ptr(), 32) };

        Ok(TxHash(c_result.data))
    }

    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError> {
        // P0-8 fix: load the initialization flag atomically (see submit_root).
        if !self.initialized.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(SettlementError::ConfigError(
                "FFI settlement adapter not initialized".to_string(),
            ));
        }

        // SAFETY: `tx.0` is a fixed-size `[u8; 32]` array; `tx.0.as_ptr()`
        // is valid for reads of exactly 32 bytes for the duration of the
        // FFI call. The C function returns a `CFinalityProof` by value
        // (a POD struct), so no aliasing concern arises on return. The C
        // library must not retain the pointer beyond the call.
        let c_result = unsafe { settlement_fetch_finality(tx.0.as_ptr()) };

        Ok(FinalityProof {
            tx_hash: TxHash(c_result.tx_hash),
            block_number: c_result.block_number,
            confirmation_count: c_result.confirmation_count,
            proof_hash: c_result.proof_hash,
        })
    }

    async fn verify_inclusion(&self, leaf: &[u8; 32], proof: &MerkleProof) -> Result<bool, SettlementError> {
        // P0-8 fix: load the initialization flag atomically (see submit_root).
        if !self.initialized.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(SettlementError::ConfigError(
                "FFI settlement adapter not initialized".to_string(),
            ));
        }

        // Flatten siblings into a contiguous byte buffer
        let sibling_bytes: Vec<u8> = proof.siblings.iter().flat_map(|s| s.iter().copied()).collect();
        let direction_bytes: Vec<u8> = proof.directions.iter().map(|&d| d as u8).collect();

        // P0-8 fix: guard the empty-vec case before passing to C. An empty
        // `Vec<u8>` produces a dangling pointer from `.as_ptr()`, which is
        // technically valid in Rust for zero-length reads but is a common
        // source of UB when the C side dereferences it unconditionally
        // (e.g., `memcpy(dst, ptr, 0)` is fine, but `ptr[0]` is not).
        // Reject up-front so the C function never sees a zero-length
        // sibling buffer.
        if sibling_bytes.is_empty() {
            return Err(SettlementError::ContractError(
                "FFI verify_inclusion rejected: empty sibling proof — \
                 Merkle proof must contain at least one sibling"
                    .to_string(),
            ));
        }
        // Similarly guard the directions buffer; it must be at least one
        // byte long if there is at least one sibling.
        if direction_bytes.is_empty() {
            return Err(SettlementError::ContractError(
                "FFI verify_inclusion rejected: empty directions buffer — \
                 must contain one byte per sibling"
                    .to_string(),
            ));
        }

        // SAFETY:
        //   - `leaf.as_ptr()` points to a fixed `[u8; 32]` borrowed for the
        //     duration of the call (caller's stack frame); valid for 32 bytes.
        //   - `sibling_bytes.as_ptr()` is valid for `sibling_count * 32`
        //     bytes (we verified `sibling_bytes` is non-empty above; the C
        //     ABI requires `sibling_count * 32` bytes — i.e. one 32-byte
        //     sibling hash per entry in `proof.siblings`).
        //   - `direction_bytes.as_ptr()` is valid for `sibling_count` bytes.
        //   - `sibling_count` is passed as `proof.siblings.len()`, which
        //     matches the number of 32-byte chunks in `sibling_bytes` and
        //     the number of bytes in `direction_bytes`.
        //   - The C function returns `CInclusionResult` by value (a POD
        //     struct), so there is no aliasing or lifetime concern on the
        //     return path. The C library must not retain any of the
        //     pointers beyond the call.
        let c_result = unsafe {
            settlement_verify_inclusion(
                leaf.as_ptr(), // leaf value provided by caller
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
        // P0-8 fix: load atomically. Previously this was a plain `bool`
        // read, which is unsound under concurrent access.
        self.initialized.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(all(test, feature = "settlement-ffi", has_settlement_lib))]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_adapter_not_initialized() {
        let adapter = FfiSettlementAdapter::new("http://localhost:8545");
        assert!(!adapter.is_live());
    }
}
