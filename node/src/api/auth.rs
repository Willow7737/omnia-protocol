//! Authentication, authorization, rate limiting, and CORS middleware
//!
//! This module provides the security middleware for the Omnia REST API:
//! - **JWT authentication** — validates `Authorization: Bearer <token>` headers
//! - **Authorized callers registry** — restricts privileged operations to known identities
//! - **Rate limiting** — token-bucket per-client rate limiter
//! - **CORS configuration** — cross-origin resource sharing via `tower-http`
//!
//! # Configuration
//!
//! | Env var                    | Purpose                                    | Default                  |
//! |----------------------------|--------------------------------------------|--------------------------|
//! | `OMNIA_JWT_SECRET`        | HMAC-SHA256 secret for signing/verifying   | *(required for JWT ops)* |
//! | `OMNIA_AUTHORIZED_CALLERS` | Comma-separated list of caller IDs         | *(empty — no privileged callers)* |
//! | `OMNIA_RATE_LIMIT_RPS`    | Max requests per second per client         | `10`                     |
//! | `OMNIA_CORS_ORIGINS`      | Comma-separated allowed origins (or `*`)   | `*` (all origins)        |

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use axum::extract::Request;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use axum::Json;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

// ---------------------------------------------------------------------------
// JWT Claims
// ---------------------------------------------------------------------------

/// JWT claims embedded in every Omnia API token.
///
/// The `sub` field identifies the caller (public key, node ID, or DID).
/// Standard `iat` / `exp` fields control token lifetime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — the caller identity (public key, node ID, or DID).
    pub sub: String,
    /// Issued-at timestamp (seconds since Unix epoch).
    pub iat: u64,
    /// Expiration timestamp (seconds since Unix epoch).
    pub exp: u64,
}

// ---------------------------------------------------------------------------
// Auth errors
// ---------------------------------------------------------------------------

/// Errors produced by authentication and authorization checks.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The `Authorization` header is missing entirely.
    #[error("missing authorization header")]
    MissingAuthHeader,

    /// The `Authorization` header does not follow the `Bearer <token>` format.
    #[error("invalid authorization header format — expected 'Bearer <token>'")]
    InvalidAuthFormat,

    /// The JWT has expired.
    #[error("token expired")]
    TokenExpired,

    /// The JWT signature or structure is invalid.
    #[error("invalid token: {0}")]
    InvalidToken(String),

    /// The caller identity is not in the [`AuthorizedCallers`] registry.
    #[error("caller '{0}' is not authorized for this operation")]
    Unauthorized(String),

    /// The `OMNIA_JWT_SECRET` environment variable is not set.
    #[error("JWT secret not configured — set OMNIA_JWT_SECRET")]
    SecretNotConfigured,

    /// Rate-limit exceeded — too many requests.
    #[error("rate limit exceeded")]
    RateLimitExceeded,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AuthError::MissingAuthHeader | AuthError::InvalidAuthFormat => (StatusCode::UNAUTHORIZED, self.to_string()),
            AuthError::TokenExpired | AuthError::InvalidToken(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            AuthError::Unauthorized(_) => (StatusCode::FORBIDDEN, self.to_string()),
            AuthError::SecretNotConfigured => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AuthError::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
        };
        let body = serde_json::json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// Authorized callers registry
// ---------------------------------------------------------------------------

/// Registry of caller identities that are permitted to perform privileged
/// operations (e.g. `mint`, `advance_epoch`).
///
/// The registry is loaded once at startup from the `OMNIA_AUTHORIZED_CALLERS`
/// environment variable (comma-separated list). If the variable is not set,
/// the registry starts empty, meaning **no** caller can perform privileged
/// operations until the list is populated.
#[derive(Debug, Clone)]
pub struct AuthorizedCallers {
    /// Set of allowed caller identity strings.
    callers: HashSet<String>,
}

impl AuthorizedCallers {
    /// Build the registry from the `OMNIA_AUTHORIZED_CALLERS` env var.
    ///
    /// The value is interpreted as a comma-separated list of caller IDs.
    /// Leading/trailing whitespace is trimmed from each entry.
    pub fn from_env() -> Self {
        let callers = std::env::var("OMNIA_AUTHORIZED_CALLERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self { callers }
    }

    /// Build the registry from an explicit list of caller IDs.
    ///
    /// This is also available via the [`FromIterator`] trait implementation.
    pub fn from_caller_ids<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self {
            callers: iter.into_iter().collect(),
        }
    }

    /// Check whether a caller identity is authorized.
    pub fn is_authorized(&self, caller_id: &str) -> bool {
        self.callers.contains(caller_id)
    }
}

impl std::iter::FromIterator<String> for AuthorizedCallers {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self {
            callers: iter.into_iter().collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Caller identity — stored in request extensions after auth
// ---------------------------------------------------------------------------

/// The authenticated caller identity, inserted into request extensions by
/// the [`require_auth`] middleware so downstream handlers can inspect it.
#[derive(Debug, Clone)]
pub struct CallerIdentity {
    /// The caller identity string (from the JWT `sub` claim).
    pub caller_id: String,
}

// ---------------------------------------------------------------------------
// JWT helpers
// ---------------------------------------------------------------------------

/// Global cache for the JWT signing secret.
///
/// `None` means the cache has not been populated yet; `Some(inner)` holds
/// the cached result of reading `OMNIA_JWT_SECRET` (where `inner` is
/// `None` if the env var is unset). The cache persists for the process
/// lifetime but can be reset in tests via \[`reset_jwt_secret_for_test`\].
static JWT_SECRET: StdMutex<Option<Option<String>>> = StdMutex::new(None);

/// Minimum acceptable length (in bytes) for `OMNIA_JWT_SECRET`.
///
/// HMAC-SHA256 keys shorter than this offer too little entropy to resist
/// offline brute force. 32 bytes matches the digest width and is the
/// value `openssl rand -hex 32` / `-base64 32` produce.
pub const MIN_JWT_SECRET_LEN: usize = 32;

/// Publicly-known or trivially-guessable JWT secrets the node refuses to
/// boot with (compared case-insensitively, after trimming). The compose
/// files historically defaulted to `omnia-testnet-jwt-secret-CHANGE-ME`,
/// so any operator who didn't override it shipped a forgeable secret
/// (AUDIT-2026-07 C11, #349). Substrings `change-me` / `changeme` are also
/// rejected separately, so variants of the placeholder are caught too.
const WEAK_JWT_SECRETS: &[&str] = &[
    "omnia-testnet-jwt-secret-change-me",
    "secret",
    "jwt-secret",
    "jwtsecret",
    "password",
    "changeme",
    "change-me",
    "test",
    "test-secret",
    "testsecret",
    "insecure",
    "default",
    "admin",
];

/// The configured `OMNIA_JWT_SECRET` is unsafe to run with.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WeakJwtSecretError {
    /// The secret is set but empty (or all whitespace).
    #[error("OMNIA_JWT_SECRET is empty — set a strong secret, e.g. `openssl rand -hex 32`")]
    Empty,
    /// The secret is a publicly-known placeholder or a trivially weak value.
    #[error(
        "OMNIA_JWT_SECRET is a publicly-known or trivially weak value — refusing to start. \
         Generate a strong, unique secret, e.g. `openssl rand -hex 32`"
    )]
    KnownWeak,
    /// The secret is shorter than [`MIN_JWT_SECRET_LEN`] bytes.
    #[error(
        "OMNIA_JWT_SECRET is too short ({0} bytes); require at least 32 bytes of entropy. \
         Generate one with `openssl rand -hex 32`"
    )]
    TooShort(usize),
}

/// Validate the strength of the configured `OMNIA_JWT_SECRET` at startup
/// (AUDIT-2026-07 C11, #349).
///
/// Returns `Ok(())` when the variable is **unset** — that is a distinct,
/// separately-handled condition (the auth middleware then rejects every
/// authenticated request; it is never an auth *bypass*). When the variable
/// is set, the value must not be empty, must not be a known-weak/placeholder
/// secret, and must be at least [`MIN_JWT_SECRET_LEN`] bytes. Callers should
/// treat an `Err` as fatal and refuse to start.
pub fn validate_jwt_secret_strength() -> Result<(), WeakJwtSecretError> {
    match std::env::var("OMNIA_JWT_SECRET") {
        Err(_) => Ok(()),
        Ok(secret) => check_jwt_secret_strength(&secret),
    }
}

/// Pure strength check for a candidate secret — factored out of
/// [`validate_jwt_secret_strength`] so it is unit-testable without touching
/// process environment variables.
fn check_jwt_secret_strength(secret: &str) -> Result<(), WeakJwtSecretError> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return Err(WeakJwtSecretError::Empty);
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("change-me") || lower.contains("changeme") || WEAK_JWT_SECRETS.contains(&lower.as_str()) {
        return Err(WeakJwtSecretError::KnownWeak);
    }
    if trimmed.len() < MIN_JWT_SECRET_LEN {
        return Err(WeakJwtSecretError::TooShort(trimmed.len()));
    }
    Ok(())
}

/// Initialise the JWT secret cache. Called once at application startup.
///
/// This must be invoked before any request hits the auth middleware so
/// that the secret is captured from the environment at a deterministic
/// point. Subsequent calls are no-ops if the cache is already populated.
pub fn init_jwt_secret() {
    let mut guard = JWT_SECRET.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(std::env::var("OMNIA_JWT_SECRET").ok());
    }
}

/// Read the JWT signing secret from the `OMNIA_JWT_SECRET` env var.
///
/// Returns `None` if the variable is not set. The value is cached so
/// that it is read at most once per process lifetime (unless explicitly
/// reset via \[`reset_jwt_secret_for_test`\] in test code).
fn jwt_secret() -> Option<String> {
    let mut guard = JWT_SECRET.lock().unwrap_or_else(|e| e.into_inner());
    match &*guard {
        Some(cached) => cached.clone(),
        None => {
            let val = std::env::var("OMNIA_JWT_SECRET").ok();
            *guard = Some(val.clone());
            val
        }
    }
}

/// Reset the JWT secret cache so the env var will be re-read on next access.
///
/// Intended for tests. This cannot be `#[cfg(test)]`-gated: that gate only
/// exists when compiling the library's own unit tests, so integration test
/// binaries under `tests/` (which link the regular library build) would get
/// a "cannot find function" compile error — exactly what broke CI on dev.
/// The function is harmless in production (it only clears a cache that is
/// repopulated from the env var on next access), so it is exposed but
/// hidden from docs.
#[doc(hidden)]
pub fn reset_jwt_secret_for_test() {
    let mut guard = JWT_SECRET.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// Create a signed JWT for the given caller identity.
///
/// The token is valid for `ttl_secs` seconds from the current time.
///
/// # Arguments
///
/// * `caller_id` — The identity string to embed as the JWT `sub` claim.
/// * `ttl_secs` — Time-to-live in seconds until the token expires.
///
/// # Errors
///
/// Returns [`AuthError::SecretNotConfigured`] if `OMNIA_JWT_SECRET` is not set.
pub fn create_token(caller_id: &str, ttl_secs: u64) -> Result<String, AuthError> {
    let secret = jwt_secret().ok_or(AuthError::SecretNotConfigured)?;
    let now = epoch_secs();
    let claims = Claims {
        sub: caller_id.to_string(),
        iat: now,
        exp: now + ttl_secs,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AuthError::InvalidToken(e.to_string()))
}

/// Validate a JWT string and return the decoded [`Claims`].
///
/// # Errors
///
/// Returns an [`AuthError`] variant if the token is invalid, expired, or the
/// secret is not configured.
pub fn validate_token(token: &str) -> Result<Claims, AuthError> {
    let secret = jwt_secret().ok_or(AuthError::SecretNotConfigured)?;
    let validation = Validation::default();
    // `validate_exp` is true by default; leeway is 60 s.
    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation)
        .map(|data| data.claims)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
            other => AuthError::InvalidToken(format!("{other:?}")),
        })
}

/// Return the current Unix timestamp in seconds.
fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Auth middleware
// ---------------------------------------------------------------------------

/// Axum middleware that requires a valid JWT in the `Authorization` header.
///
/// On success the decoded [`CallerIdentity`] is stored in request extensions
/// so handlers can retrieve it with `req.extensions().get::<CallerIdentity>()`.
///
/// If `OMNIA_JWT_SECRET` is not set, the middleware **rejects the request**
/// with 503 Service Unavailable instead of silently bypassing authentication.
/// This prevents a critical auth bypass in production when the environment
/// variable is accidentally unset.
///
/// On failure an appropriate HTTP error response is returned immediately.
pub async fn require_auth(mut req: Request, next: Next) -> Response {
    // If no JWT secret is configured, reject the request instead of
    // silently bypassing authentication. This prevents a critical auth
    // bypass in production when OMNIA_JWT_SECRET is accidentally unset.
    let secret = match jwt_secret() {
        Some(s) => s,
        None => {
            // Reject all requests when JWT secret is not configured
            // This prevents silent auth bypass in production
            tracing::error!("OMNIA_JWT_SECRET not set - rejecting authenticated request");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "authentication not configured",
                    "message": "OMNIA_JWT_SECRET environment variable must be set"
                })),
            )
                .into_response();
        }
    };

    let auth_header = req.headers().get(AUTHORIZATION).and_then(|v| v.to_str().ok());

    let token = match auth_header {
        None => return AuthError::MissingAuthHeader.into_response(),
        Some(header) => {
            if let Some(tok) = header.strip_prefix("Bearer ") {
                tok
            } else {
                return AuthError::InvalidAuthFormat.into_response();
            }
        }
    };

    let validation = Validation::default();
    match decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation) {
        Ok(token_data) => {
            req.extensions_mut().insert(CallerIdentity {
                caller_id: token_data.claims.sub,
            });
            next.run(req).await
        }
        Err(e) => match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired.into_response(),
            other => AuthError::InvalidToken(format!("{other:?}")).into_response(),
        },
    }
}

// ---------------------------------------------------------------------------
// Authorization helper (for use in handlers)
// ---------------------------------------------------------------------------

/// Check whether the caller in the request extensions is authorized for a
/// privileged operation, returning [`AuthError::Unauthorized`] if not.
///
/// # Arguments
///
/// * `extensions` — the request extensions (containing [`CallerIdentity`])
/// * `authorized` — the [`AuthorizedCallers`] registry
///
/// # Returns
///
/// `Ok(())` if the caller is authorized, `Err(AuthError)` otherwise.
pub fn require_privileged(
    extensions: &axum::http::Extensions,
    authorized: &AuthorizedCallers,
) -> Result<(), AuthError> {
    let caller_id = extensions
        .get::<CallerIdentity>()
        .map(|ci| ci.caller_id.as_str())
        .unwrap_or("");

    if authorized.is_authorized(caller_id) {
        Ok(())
    } else {
        Err(AuthError::Unauthorized(caller_id.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Token-bucket rate limiter
// ---------------------------------------------------------------------------

/// A single token bucket for one client, with last-access tracking
/// for stale-bucket eviction.
#[derive(Debug)]
struct TrackedBucket {
    /// The underlying token bucket.
    bucket: Bucket,
    /// Timestamp of the last access (consume or refill check).
    last_access: Instant,
}

/// A single token bucket for one client.
#[derive(Debug)]
struct Bucket {
    /// Current number of tokens available.
    tokens: f64,
    /// Maximum number of tokens the bucket can hold.
    max_tokens: f64,
    /// Token refill rate (tokens per second).
    refill_rate: f64,
    /// Timestamp of the last refill.
    last_refill: Instant,
}

impl Bucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time, then attempt to consume one token.
    ///
    /// Returns `true` if a token was consumed, `false` if the bucket is empty.
    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;

        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Per-client token-bucket rate limiter with stale-bucket eviction.
///
/// Each distinct client key (typically an IP address) gets its own bucket.
/// Buckets are lazily created on first request and refilled over time.
///
/// Stale buckets (no access for > 1 hour) are evicted when the total
/// number of buckets exceeds 1 000, preventing unbounded memory growth
/// from clients that connect once and never return.
#[derive(Debug)]
pub struct RateLimiter {
    /// Map from client key to its tracked token bucket.
    buckets: Mutex<HashMap<String, TrackedBucket>>,
    /// Maximum burst size (tokens per bucket).
    max_tokens: f64,
    /// Sustained refill rate in tokens per second.
    refill_rate: f64,
    /// Whether to trust X-Real-IP header for client identification.
    /// Only enable this when running behind a known, trusted proxy.
    trust_proxy_headers: bool,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// # Arguments
    ///
    /// * `max_tokens` — maximum burst size (bucket capacity)
    /// * `refill_rate` — sustained rate in requests per second
    pub fn new(max_tokens: u64, refill_rate: u64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_tokens: max_tokens as f64,
            refill_rate: refill_rate as f64,
            trust_proxy_headers: false,
        }
    }

    /// Create a new rate limiter with configurable proxy header trust.
    ///
    /// # Arguments
    ///
    /// * `max_tokens` — maximum burst size (bucket capacity)
    /// * `refill_rate` — sustained rate in requests per second
    /// * `trust_proxy_headers` — whether to trust X-Real-IP header
    pub fn with_proxy_trust(max_tokens: u64, refill_rate: u64, trust_proxy_headers: bool) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_tokens: max_tokens as f64,
            refill_rate: refill_rate as f64,
            trust_proxy_headers,
        }
    }

    /// Build a [`RateLimiter`] from the `OMNIA_RATE_LIMIT_RPS` env var.
    ///
    /// Defaults to 10 requests/second with a burst of 20 if the variable
    /// is not set or cannot be parsed. Proxy header trust is controlled
    /// by `OMNIA_TRUST_PROXY_HEADERS` (defaults to `false`).
    pub fn from_env() -> Self {
        let rps = std::env::var("OMNIA_RATE_LIMIT_RPS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10);
        let trust_proxy_headers = std::env::var("OMNIA_TRUST_PROXY_HEADERS")
            .ok()
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        if trust_proxy_headers {
            tracing::warn!("X-Real-IP header trust enabled — only use behind a trusted proxy");
        }
        // Burst is 2× the sustained rate
        Self::with_proxy_trust(rps * 2, rps, trust_proxy_headers)
    }

    /// Evict buckets that have not been accessed for over 1 hour.
    ///
    /// This prevents unbounded memory growth from one-off clients.
    fn evict_stale_buckets(buckets: &mut HashMap<String, TrackedBucket>) {
        let stale_threshold = std::time::Duration::from_secs(3600); // 1 hour
        buckets.retain(|_, tb| tb.last_access.elapsed() < stale_threshold);
    }

    /// Attempt to consume one token for the given client key.
    ///
    /// Returns `true` if the request should be allowed, `false` if the
    /// rate limit has been exceeded.
    pub async fn try_consume(&self, client_key: &str) -> bool {
        let mut buckets = self.buckets.lock().await;

        // Evict stale buckets periodically to prevent unbounded memory growth
        if buckets.len() > 1000 {
            Self::evict_stale_buckets(&mut buckets);
        }

        let tb = buckets.entry(client_key.to_string()).or_insert_with(|| TrackedBucket {
            bucket: Bucket::new(self.max_tokens, self.refill_rate),
            last_access: Instant::now(),
        });
        tb.last_access = Instant::now();
        tb.bucket.try_consume()
    }
}

/// Axum middleware that enforces per-client rate limiting.
///
/// The client key is derived (in priority order) from:
/// 1. The actual peer IP from `ConnectInfo<SocketAddr>` (when available)
/// 2. The `X-Real-IP` header (set by a trusted reverse proxy)
/// 3. The fallback key `"unauthenticated"`
///
/// **Note**: `X-Forwarded-For` is intentionally **not** used because it can
/// be spoofed by clients, causing all malicious requests to share a single
/// rate-limit bucket and bypass per-client limits.
///
/// The [`RateLimiter`] must be provided via an `Extension` layer.
///
/// Returns **429 Too Many Requests** when the rate limit is exceeded.
pub async fn rate_limit_middleware(
    Extension(limiter): Extension<Arc<RateLimiter>>,
    req: Request,
    next: Next,
) -> Response {
    // Try to get actual peer IP from connection info first.
    // Only trust X-Real-IP if explicitly enabled (behind a known proxy).
    // Do NOT trust X-Forwarded-For from untrusted sources as it can be
    // spoofed, causing all requests to share a single rate-limit bucket.
    let client_key = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .or_else(|| {
            if limiter.trust_proxy_headers {
                req.headers()
                    .get("x-real-ip")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unauthenticated".to_string());

    if limiter.try_consume(&client_key).await {
        next.run(req).await
    } else {
        AuthError::RateLimitExceeded.into_response()
    }
}

// ---------------------------------------------------------------------------
// CORS helper
// ---------------------------------------------------------------------------

/// Build a [`CorsLayer`] for the Omnia REST API, configurable via the
/// `OMNIA_CORS_ORIGINS` environment variable.
///
/// - When `OMNIA_CORS_ORIGINS` is set to `*` (or unset), all origins are
///   allowed **with a warning** — suitable only for development.
/// - When set to a comma-separated list of origins (e.g.
///   `https://app.example.com,https://admin.example.com`), only those
///   origins are permitted.
///
/// Defaults:
/// - **Methods**: `GET`, `POST`, `PUT`, `DELETE`
/// - **Headers**: any (via `tower_http::cors::Any`)
/// - **Max age**: 1 hour
pub fn default_cors_layer() -> CorsLayer {
    let allowed_origins = std::env::var("OMNIA_CORS_ORIGINS").unwrap_or_else(|_| "*".to_string());

    if allowed_origins == "*" {
        tracing::warn!(
            "SECURITY: CORS allows all origins (*) — this is suitable only for local development. \
             Set OMNIA_CORS_ORIGINS to a comma-separated list of allowed origins for production."
        );
        CorsLayer::permissive()
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
            ])
            .allow_headers([AUTHORIZATION, CONTENT_TYPE])
            .max_age(std::time::Duration::from_secs(3600))
    } else {
        let origins: Vec<_> = allowed_origins
            .split(',')
            .filter_map(|o| o.trim().parse().ok())
            .collect();
        if origins.is_empty() {
            tracing::warn!("No valid CORS origins parsed — using restrictive defaults");
        }
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
            .max_age(std::time::Duration::from_secs(3600))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Global mutex to serialize tests that read/write `OMNIA_JWT_SECRET`.
    /// Without this, parallel test execution causes race conditions where
    /// one test sets the var and another removes it mid-assertion.
    static JWT_SECRET_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_authorized_callers_from_caller_ids() {
        let callers = AuthorizedCallers::from_caller_ids(vec!["node-1".to_string(), "node-2".to_string()]);
        assert!(callers.is_authorized("node-1"));
        assert!(callers.is_authorized("node-2"));
        assert!(!callers.is_authorized("node-3"));
    }

    #[test]
    fn test_authorized_callers_from_iterator_trait() {
        let callers: AuthorizedCallers = vec!["a".to_string(), "b".to_string()].into_iter().collect();
        assert!(callers.is_authorized("a"));
        assert!(callers.is_authorized("b"));
        assert!(!callers.is_authorized("c"));
    }

    #[test]
    fn test_authorized_callers_empty() {
        let callers = AuthorizedCallers::from_caller_ids(vec![]);
        assert!(!callers.is_authorized("anyone"));
    }

    #[test]
    fn test_create_and_validate_token() {
        // Hold the lock for the entire test so no other test touches the
        // JWT_SECRET env var while we depend on it.
        let _lock = JWT_SECRET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_jwt_secret_for_test();
        std::env::set_var("OMNIA_JWT_SECRET", "test-secret-key");
        let token = create_token("caller-42", 3600).expect("create token");
        let claims = validate_token(&token).expect("validate token");
        assert_eq!(claims.sub, "caller-42");
        std::env::remove_var("OMNIA_JWT_SECRET");
        reset_jwt_secret_for_test();
    }

    #[test]
    fn test_validate_expired_token() {
        // Encode/decode directly — no env-var dependency, no lock needed.
        let secret = "test-expired-secret";
        let claims = Claims {
            sub: "expired-caller".to_string(),
            iat: 1,
            exp: 1, // 1970-01-01 — definitely expired
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("encode");
        let result = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        );
        assert!(result.is_err(), "Token with exp=1 should be expired");
    }

    #[test]
    fn test_create_token_no_secret() {
        let _lock = JWT_SECRET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Remove the var if it was set by a previous test, then verify
        // that create_token returns SecretNotConfigured.
        std::env::remove_var("OMNIA_JWT_SECRET");
        reset_jwt_secret_for_test();
        let result = create_token("caller-42", 3600);
        assert!(
            matches!(result, Err(AuthError::SecretNotConfigured)),
            "Expected SecretNotConfigured when OMNIA_JWT_SECRET is unset"
        );
        reset_jwt_secret_for_test();
    }

    #[test]
    fn test_validate_invalid_token() {
        let _lock = JWT_SECRET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OMNIA_JWT_SECRET", "test-secret-key");
        reset_jwt_secret_for_test();
        let result = validate_token("not.a.valid-token");
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
        std::env::remove_var("OMNIA_JWT_SECRET");
        reset_jwt_secret_for_test();
    }

    #[tokio::test]
    async fn test_rate_limiter_allows_within_limit() {
        let limiter = RateLimiter::new(5, 5);
        for _ in 0..5 {
            assert!(limiter.try_consume("client-a").await);
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(3, 1);
        for _ in 0..3 {
            assert!(limiter.try_consume("client-a").await);
        }
        // 4th request should be blocked
        assert!(!limiter.try_consume("client-a").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_per_client_isolation() {
        let limiter = RateLimiter::new(2, 1);
        assert!(limiter.try_consume("client-a").await);
        assert!(limiter.try_consume("client-a").await);
        // client-a is exhausted, but client-b should be fine
        assert!(limiter.try_consume("client-b").await);
    }

    #[test]
    fn test_require_privileged_authorized() {
        let callers = AuthorizedCallers::from_caller_ids(vec!["admin".to_string()]);
        let mut extensions = axum::http::Extensions::new();
        extensions.insert(CallerIdentity {
            caller_id: "admin".to_string(),
        });
        assert!(require_privileged(&extensions, &callers).is_ok());
    }

    #[test]
    fn test_require_privileged_unauthorized() {
        let callers = AuthorizedCallers::from_caller_ids(vec!["admin".to_string()]);
        let mut extensions = axum::http::Extensions::new();
        extensions.insert(CallerIdentity {
            caller_id: "random-user".to_string(),
        });
        let result = require_privileged(&extensions, &callers);
        assert!(matches!(result, Err(AuthError::Unauthorized(id)) if id == "random-user"));
    }

    #[test]
    fn test_require_privileged_no_identity() {
        let callers = AuthorizedCallers::from_caller_ids(vec!["admin".to_string()]);
        let extensions = axum::http::Extensions::new();
        let result = require_privileged(&extensions, &callers);
        assert!(matches!(result, Err(AuthError::Unauthorized(id)) if id.is_empty()));
    }

    #[test]
    fn test_default_cors_layer() {
        let _layer = default_cors_layer();
        // If this compiles and doesn't panic, the CORS layer is valid.
    }

    // ---- AUDIT-2026-07 C11 (#349): weak JWT secret denylist ----

    #[test]
    fn test_check_jwt_secret_rejects_compose_default() {
        // The exact publicly-known compose default must be rejected.
        assert_eq!(
            check_jwt_secret_strength("omnia-testnet-jwt-secret-CHANGE-ME"),
            Err(WeakJwtSecretError::KnownWeak)
        );
    }

    #[test]
    fn test_check_jwt_secret_rejects_changeme_variants_case_insensitive() {
        for weak in [
            "CHANGE-ME",
            "changeme",
            "please-CHANGEME-now-xxxxxxxxxxxxxxxxxxxx",
            "my-change-me-secret-value-still-weak-yy",
            "  Secret  ",
            "PASSWORD",
            "admin",
        ] {
            assert_eq!(
                check_jwt_secret_strength(weak),
                Err(WeakJwtSecretError::KnownWeak),
                "expected {weak:?} to be rejected as known-weak"
            );
        }
    }

    #[test]
    fn test_check_jwt_secret_rejects_empty() {
        assert_eq!(check_jwt_secret_strength(""), Err(WeakJwtSecretError::Empty));
        assert_eq!(check_jwt_secret_strength("   "), Err(WeakJwtSecretError::Empty));
    }

    #[test]
    fn test_check_jwt_secret_rejects_short() {
        // 31 bytes: one under the floor, and not otherwise weak.
        let short = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d"; // 31 chars
        assert_eq!(short.len(), 31);
        assert_eq!(check_jwt_secret_strength(short), Err(WeakJwtSecretError::TooShort(31)));
    }

    #[test]
    fn test_check_jwt_secret_accepts_strong() {
        // A long random hex secret as `openssl rand -hex 32` would produce.
        let strong = "9f8c1e2d3a4b5c6d7e8f90a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8091a2b3";
        assert!(strong.len() >= MIN_JWT_SECRET_LEN);
        assert_eq!(check_jwt_secret_strength(strong), Ok(()));
    }

    #[test]
    fn test_validate_jwt_secret_strength_ok_when_unset() {
        // Unset is not a weak-secret error (handled separately by the
        // middleware, which rejects authenticated requests).
        let _lock = JWT_SECRET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OMNIA_JWT_SECRET");
        assert_eq!(validate_jwt_secret_strength(), Ok(()));
    }

    #[test]
    fn test_validate_jwt_secret_strength_rejects_env_default() {
        let _lock = JWT_SECRET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OMNIA_JWT_SECRET", "omnia-testnet-jwt-secret-CHANGE-ME");
        assert_eq!(validate_jwt_secret_strength(), Err(WeakJwtSecretError::KnownWeak));
        std::env::remove_var("OMNIA_JWT_SECRET");
    }
}
