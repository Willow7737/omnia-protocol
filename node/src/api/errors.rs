//! Structured error reporting endpoint.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

use crate::state::AppState;

/// Structured error code for API responses.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Internal processing error
    InternalError,
    /// Invalid request parameters
    InvalidRequest,
    /// Resource not found
    NotFound,
    /// Rate limit exceeded
    RateLimited,
    /// Service unavailable
    ServiceUnavailable,
}

/// Structured error response.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    /// Machine-readable error code
    pub code: ErrorCode,
    /// Human-readable error message (no internal details)
    pub message: String,
    /// Request ID for tracing
    pub request_id: Option<String>,
}

/// GET /api/v1/errors — Returns supported error codes and documentation.
///
/// Rate-limited to prevent abuse. Does not leak internal details
/// (no stack traces, file paths, or system info).
pub async fn error_codes(
    State(_state): State<Arc<AppState>>,
) -> (StatusCode, Json<Vec<ErrorResponse>>) {
    let codes = vec![
        ErrorResponse {
            code: ErrorCode::InternalError,
            message: "An internal processing error occurred".to_string(),
            request_id: None,
        },
        ErrorResponse {
            code: ErrorCode::InvalidRequest,
            message: "The request parameters are invalid".to_string(),
            request_id: None,
        },
        ErrorResponse {
            code: ErrorCode::NotFound,
            message: "The requested resource was not found".to_string(),
            request_id: None,
        },
        ErrorResponse {
            code: ErrorCode::RateLimited,
            message: "Rate limit exceeded, please retry later".to_string(),
            request_id: None,
        },
        ErrorResponse {
            code: ErrorCode::ServiceUnavailable,
            message: "The service is temporarily unavailable".to_string(),
            request_id: None,
        },
    ];
    (StatusCode::OK, Json(codes))
}
