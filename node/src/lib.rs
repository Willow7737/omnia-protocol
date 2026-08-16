//! # Omnia Node Library
//!
//! This crate provides the Omnia Protocol node implementation as both
//! a library (for integration testing and embedding) and a binary
//! (for running a standalone node).
//!
//! # Modules
//!
//! - [`config`] — CLI argument parsing and configuration validation
//! - [`state`] — Shared application state and Prometheus metrics
//! - [`http`] — HTTP server setup with health, metrics, and API routes
//! - [`api`] — REST API handlers for events, shards, governance, economics

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// C-2 fix (audit v0.1.68): the previous `#![allow(deprecated)]` suppressed
// warnings from the (now-removed) crate-level `#![deprecated]` annotation on
// omnia-substrate. With that annotation removed, deprecated warnings are
// allowed to surface again so we catch accidental use of deprecated APIs.

pub mod api;
pub mod config;
pub mod http;
pub mod payment_worker;
pub mod pipeline;
pub mod state;
