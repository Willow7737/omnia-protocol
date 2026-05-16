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

pub mod api;
pub mod config;
pub mod http;
pub mod state;
