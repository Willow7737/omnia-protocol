//! # Omnia Benches — Unified benchmark suite for the Omnia Protocol
//!
//! This crate consolidates all benchmarks into a single location,
//! making it easy to run comprehensive performance tests across
//! all protocol layers. Benchmarks use criterion for statistical
//! rigor and iai-callgrind for deterministic hot-path profiling.
//!
//! ## Benchmark categories
//!
//! - **throughput**: Core event processing, graph insertion, vector clock merge,
//!   slashing, and VRF operations (criterion-based)
//! - **zk_benchmarks**: ZK-SNARK proof generation, verification, Merkle tree,
//!   and trusted setup (criterion-based)
//! - **hot_path_iai**: Deterministic callgrind profiling for hot-path functions
//!   (iai-callgrind-based)
