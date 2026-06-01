//! Build script for omnia-adapters.
//!
//! Detects the Rust compiler version and enables conditional compilation
//! for feature-gated code that requires a newer compiler than the MSRV.
//!
//! # Why build.rs instead of Cargo features?
//!
//! Cargo features are resolved at the *workspace* level — if any crate in the
//! workspace enables a feature, every crate sees it as enabled. This makes it
//! impossible to conditionally compile code based on the *host* compiler
//! version, because feature resolution cannot inspect `rustc --version`.
//! A build script, by contrast, runs on the host before compilation and can
//! probe the compiler version (or the filesystem for pre-compiled libraries)
//! and emit `cargo:rustc-cfg=…` directives that are scoped to *this* crate
//! only. This is the only reliable way to gate code on rustc version or the
//! presence of an external static library without polluting the workspace-wide
//! feature graph.

fn main() {
    // Detect rustc version for conditional compilation.
    // The `ethereum-live` feature requires alloy which needs rustc >= 1.91.
    // When the compiler is new enough, we set `rustc_version_compatible`
    // so that live adapter code can compile.
    //
    // NOTE: This is informational only — cargo features are the primary
    // gating mechanism. The `rustc_version_compatible` cfg is used for
    // compile-time assertions and documentation, not for automatically
    // enabling features.
    if let Ok(version) = rustc_version::version() {
        if version >= semver::Version::new(1, 91, 0) {
            println!("cargo:rustc-cfg=rustc_version_compatible");
        }
        // Print version for debugging
        println!("cargo:rustc-env=OMNIA_RUSTC_VERSION={version}");
    }

    // Enable FFI linking if pre-compiled library exists.
    //
    // We emit a custom cfg `has_settlement_lib` that the FFI code uses
    // in addition to the `settlement-ffi` Cargo feature. This two-gate
    // approach prevents linker errors when `--all-features` enables
    // `settlement-ffi` but `libsettlement.a` is not present (e.g., on
    // CI runners without the pre-compiled C library).
    //
    // The `settlement-ffi` feature alone allows the C ABI types to
    // compile (for documentation and type-checking), but the `extern "C"`
    // block and `FfiSettlementAdapter` impl that reference the FFI
    // symbols require `has_settlement_lib` as well.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let settlement_lib = std::path::Path::new(&manifest_dir).join("lib/libsettlement.a");
    let settlement_lib_win = std::path::Path::new(&manifest_dir).join("lib/settlement.lib");
    if settlement_lib.exists() || settlement_lib_win.exists() {
        println!("cargo:rustc-cfg=has_settlement_lib");
        // REMOVED: Force-enabling features from build scripts violates Cargo conventions.
        // The `settlement-ffi` feature must be explicitly enabled via --features or Cargo.toml.
        // println!("cargo:rustc-cfg=feature=\"settlement-ffi\"");
        println!("cargo:rustc-link-search=native={}/lib", manifest_dir);
        println!("cargo:rustc-link-lib=static=settlement");
    }

    // Ensure build script re-runs on changes
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=lib/libsettlement.a");
    println!("cargo:rerun-if-changed=lib/settlement.lib");
}
