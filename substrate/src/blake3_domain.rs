//! Re-export of blake3 domain-separated hashing from `omnia-primitives`.
//!
//! This module provides backward compatibility for code that imports
//! the blake3 hash domain function via `use crate::blake3_domain::...`.

pub use omnia_primitives::blake3_domain::blake3_hash_domain;
