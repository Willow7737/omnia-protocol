//! Build script for omnia-adapters.
//!
//! Detects the Rust compiler version and enables conditional compilation
//! for feature-gated code that requires a newer compiler than the MSRV.

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

    // Enable FFI if pre-compiled library exists
    if std::path::Path::new("lib/libsettlement.a").exists() {
        println!("cargo:rustc-cfg=feature=\"settlement-ffi\"");
        println!("cargo:rustc-link-search=native=lib");
        println!("cargo:rustc-link-lib=static=settlement");
    }

    // Ensure build script re-runs on changes
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=lib/libsettlement.a");
}
