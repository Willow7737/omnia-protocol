//! Bridge operator trait + Ghana mobile money provider adapters (Sprint 5)
//!
//! This module provides a provider-agnostic abstraction for mobile money
//! bridge operations. Ghana's mobile payment ecosystem has three major
//! providers:
//!
//! | Provider | Network | Short Code | Market Share |
//! |----------|---------|------------|-------------|
//! | MTN      | MTN     | *170#     | ~60%        |
//! | Telecel  | Telecel | *110#     | ~25%        |
//! | AT       | AT      | *505#     | ~15%        |
//!
//! ## Architecture
//!
//! ```
//! ┌──────────────────────────────────────────┐
//! │           Omnia Protocol                 │
//! │  ┌────────────────────────────────────┐  │
//! │  │      BridgeOperator trait           │  │
//! │  │  - initiate_payment()              │  │
//! │  │  - query_status()                  │  │
//! │  │  - process_callback()              │  │
//! │  │  - refund()                        │  │
//! │  │  - health_check()                  │  │
//! │  └─────────────┬──────────────────────┘  │
//! │                │                          │
//! │    ┌───────────┼───────────┐              │
//! │    ▼           ▼           ▼              │
//! │ ┌──────┐  ┌────────┐  ┌──────┐           │
//! │ │ MTN  │  │Telecel │  │  AT  │           │
//! │ │Adapter│ │ Adapter │ │Adapter│           │
//! │ └──┬───┘  └───┬────┘  └──┬───┘           │
//! └────┼──────────┼──────────┼───────────────┘
//!      │          │          │
//!      ▼          ▼          ▼
//!   MTN API   Telecel API  AT API
//! ```
//!
//! ## Security Model
//!
//! - All API calls use mTLS + API key authentication
//! - Callbacks are verified via HMAC-SHA256
//! - Payments are idempotent (order ID deduplication)
//! - Circuit breaker pattern prevents cascade failures (§15)

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

// ------------------------------------------------------------------
// Provider identifiers
// ------------------------------------------------------------------

/// Supported mobile money providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MobileProvider {
    /// MTN Mobile Money (MoMo) — ~60% Ghana market share.
    Mtn,
    /// Telecel (formerly Vodafone Cash) — ~25% market share.
    Telecel,
    /// AT (AirtelTigo) — ~15% market share.
    At,
}

impl MobileProvider {
    /// Get the USSD short code for this provider.
    pub fn ussd_code(&self) -> &'static str {
        match self {
            MobileProvider::Mtn => "*170#",
            MobileProvider::Telecel => "*110#",
            MobileProvider::At => "*505#",
        }
    }

    /// Get the human-readable provider name.
    pub fn name(&self) -> &'static str {
        match self {
            MobileProvider::Mtn => "MTN Mobile Money",
            MobileProvider::Telecel => "Telecel Cash",
            MobileProvider::At => "AT Money",
        }
    }

    /// Parse a provider from a string identifier (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mtn" | "mtn_momo" => Some(MobileProvider::Mtn),
            "telecel" | "vodafone" | "vodafone_cash" => Some(MobileProvider::Telecel),
            "at" | "airteltigo" => Some(MobileProvider::At),
            _ => None,
        }
    }
}

impl std::fmt::Display for MobileProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ------------------------------------------------------------------
// Bridge types
// ------------------------------------------------------------------

/// Status of a bridge payment order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgePaymentStatus {
    /// Payment has been initiated but not yet confirmed.
    Pending,
    /// Payment confirmed by the mobile money provider.
    Confirmed,
    /// Payment failed (insufficient funds, timeout, etc.).
    Failed,
    /// Payment was refunded back to the user.
    Refunded,
    /// Payment is being reversed (intermediate state).
    Reversing,
}

/// A unique payment order on the bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgePaymentOrder {
    /// Unique order identifier (UUID v4 or similar).
    pub order_id: String,
    /// The mobile money provider.
    pub provider: MobileProvider,
    /// Recipient phone number (E.164 format, e.g., "+233XXXXXXXXX").
    pub phone_number: String,
    /// Amount in GHS (Ghanaian Cedi minor units — pesewas).
    pub amount_ghs_pesewas: u64,
    /// Amount in OMNIA tokens.
    pub amount_omnia: u64,
    /// Current status.
    pub status: BridgePaymentStatus,
    /// Unix timestamp (milliseconds) when the order was created.
    pub created_at: u64,
    /// Unix timestamp (milliseconds) of last status update.
    pub updated_at: u64,
    /// Provider-specific transaction reference (set after initiation).
    pub provider_ref: Option<String>,
    /// OMNIA account that initiated the payment.
    pub from_account: [u8; 32],
}

/// Configuration for a bridge provider adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// The mobile money provider.
    pub provider: MobileProvider,
    /// Base URL for the provider's API.
    pub api_base_url: String,
    /// API key for authentication.
    pub api_key: String,
    /// HMAC secret for callback verification.
    pub callback_secret: String,
    /// Request timeout in milliseconds.
    pub timeout_ms: u64,
    /// Maximum number of retries for transient failures.
    pub max_retries: u32,
    /// Whether this adapter is enabled.
    pub enabled: bool,
}

impl ProviderConfig {
    /// Get the request timeout as a `Duration`.
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}

// ------------------------------------------------------------------
// Circuit Breaker (§15)
// ------------------------------------------------------------------

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Circuit is closed — requests flow normally.
    Closed,
    /// Circuit is open — requests are rejected immediately.
    Open,
    /// Circuit is half-open — a single probe request is allowed.
    HalfOpen,
}

/// A circuit breaker that prevents cascade failures in bridge operations.
///
/// Implements the standard circuit breaker pattern:
/// 1. **Closed**: Requests flow. Consecutive failures increment a counter.
/// 2. **Open**: After `failure_threshold` consecutive failures, the circuit
///    opens. All requests are rejected with `BridgeError::CircuitOpen`.
/// 3. **Half-Open**: After `reset_timeout`, the circuit moves to half-open.
///    A single probe request is allowed. If it succeeds, the circuit closes.
///    If it fails, it reopens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreaker {
    /// Current state of the circuit breaker.
    pub state: CircuitState,
    /// Number of consecutive failures.
    pub consecutive_failures: u32,
    /// Number of consecutive successes (used in half-open state).
    pub consecutive_successes: u32,
    /// Failure threshold — circuit opens after this many consecutive failures.
    pub failure_threshold: u32,
    /// Success threshold in half-open state to close the circuit.
    pub success_threshold: u32,
    /// Reset timeout in milliseconds — how long the circuit stays open.
    pub reset_timeout_ms: u64,
    /// Timestamp (ms) when the circuit opened.
    pub opened_at: Option<u64>,
    /// Total requests recorded.
    pub total_requests: u64,
    /// Total failures recorded.
    pub total_failures: u64,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(5, 1, 30_000) // 5 failures, 1 success to reset, 30s timeout
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    pub fn new(failure_threshold: u32, success_threshold: u32, reset_timeout_ms: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            consecutive_successes: 0,
            failure_threshold,
            success_threshold,
            reset_timeout_ms,
            opened_at: None,
            total_requests: 0,
            total_failures: 0,
        }
    }

    /// Check if a request is allowed under the current circuit state.
    ///
    /// In `Closed` state, always allows. In `Open` state, checks if
    /// the reset timeout has elapsed (transitioning to `HalfOpen` if so).
    /// In `HalfOpen` state, allows a single probe request.
    pub fn allow_request(&mut self, now_ms: u64) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(opened_at) = self.opened_at {
                    if now_ms.saturating_sub(opened_at) >= self.reset_timeout_ms {
                        self.state = CircuitState::HalfOpen;
                        self.consecutive_successes = 0;
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful request.
    pub fn record_success(&mut self) {
        self.total_requests = self.total_requests.saturating_add(1);
        self.consecutive_failures = 0;
        match self.state {
            CircuitState::Closed => {
                // Stay closed
            }
            CircuitState::HalfOpen => {
                self.consecutive_successes = self.consecutive_successes.saturating_add(1);
                if self.consecutive_successes >= self.success_threshold {
                    self.state = CircuitState::Closed;
                    self.opened_at = None;
                }
            }
            CircuitState::Open => {
                // Should not happen, but handle gracefully
            }
        }
    }

    /// Record a failed request.
    pub fn record_failure(&mut self, now_ms: u64) {
        self.total_requests = self.total_requests.saturating_add(1);
        self.total_failures = self.total_failures.saturating_add(1);
        self.consecutive_successes = 0;
        match self.state {
            CircuitState::Closed => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                if self.consecutive_failures >= self.failure_threshold {
                    self.state = CircuitState::Open;
                    self.opened_at = Some(now_ms);
                }
            }
            CircuitState::HalfOpen => {
                // Failure in half-open → immediately reopen
                self.state = CircuitState::Open;
                self.opened_at = Some(now_ms);
            }
            CircuitState::Open => {
                // Already open, just count
            }
        }
    }

    /// Force-reset the circuit breaker to closed state.
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.consecutive_failures = 0;
        self.consecutive_successes = 0;
        self.opened_at = None;
    }

    /// Get the current state.
    pub fn state(&self) -> CircuitState {
        self.state
    }
}

// ------------------------------------------------------------------
// Bridge errors
// ------------------------------------------------------------------

/// Errors that can occur during bridge operations.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeError {
    /// The circuit breaker is open — requests are rejected.
    CircuitOpen,
    /// The mobile money provider returned an error.
    ProviderError {
        /// The mobile money provider that returned the error.
        provider: MobileProvider,
        /// The provider-specific error code.
        code: String,
        /// The provider-specific error message.
        message: String,
    },
    /// Invalid phone number format.
    InvalidPhoneNumber(String),
    /// Amount is below the minimum allowed.
    AmountBelowMinimum {
        /// The minimum allowed amount in pesewas.
        min_pesewas: u64,
        /// The requested amount in pesewas.
        requested: u64,
    },
    /// Amount exceeds the maximum allowed.
    AmountAboveMaximum {
        /// The maximum allowed amount in pesewas.
        max_pesewas: u64,
        /// The requested amount in pesewas.
        requested: u64,
    },
    /// Payment order not found.
    OrderNotFound(String),
    /// Duplicate order ID.
    DuplicateOrder(String),
    /// Callback verification failed (invalid HMAC).
    CallbackVerificationFailed,
    /// Timeout waiting for provider response.
    Timeout,
    /// The provider is not configured or enabled.
    ProviderNotConfigured(MobileProvider),
    /// Rate limit exceeded for this provider.
    RateLimited {
        /// The provider that rate-limited the request.
        provider: MobileProvider,
        /// Suggested retry delay in milliseconds.
        retry_after_ms: u64,
    },
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::CircuitOpen => write!(f, "Circuit breaker is open"),
            BridgeError::ProviderError {
                provider,
                code,
                message,
            } => {
                write!(f, "Provider {provider} error ({code}): {message}")
            }
            BridgeError::InvalidPhoneNumber(phone) => {
                write!(f, "Invalid phone number: {phone}")
            }
            BridgeError::AmountBelowMinimum { min_pesewas, requested } => {
                write!(f, "Amount {requested} pesewas is below minimum {min_pesewas}")
            }
            BridgeError::AmountAboveMaximum { max_pesewas, requested } => {
                write!(f, "Amount {requested} pesewas exceeds maximum {max_pesewas}")
            }
            BridgeError::OrderNotFound(id) => write!(f, "Order not found: {id}"),
            BridgeError::DuplicateOrder(id) => write!(f, "Duplicate order: {id}"),
            BridgeError::CallbackVerificationFailed => {
                write!(f, "Callback HMAC verification failed")
            }
            BridgeError::Timeout => write!(f, "Provider request timed out"),
            BridgeError::ProviderNotConfigured(p) => {
                write!(f, "Provider not configured: {p}")
            }
            BridgeError::RateLimited {
                provider,
                retry_after_ms,
            } => {
                write!(f, "Rate limited by {provider}, retry after {retry_after_ms}ms")
            }
        }
    }
}

impl std::error::Error for BridgeError {}

// ------------------------------------------------------------------
// BridgeOperator trait
// ------------------------------------------------------------------

/// The core bridge operator trait.
///
/// This trait defines the interface for interacting with mobile money
/// providers. Each provider (MTN, Telecel, AT) implements this trait.
/// The bridge operator handles:
///
/// 1. **Initiate payment** — Send GHS to a phone number, funded by OMNIA tokens
/// 2. **Query status** — Check the current status of a payment order
/// 3. **Process callback** — Handle async payment confirmations from providers
/// 4. **Refund** — Return OMNIA tokens when a payment fails
/// 5. **Health check** — Verify the provider connection is healthy
///
/// ## Idempotency
///
/// All mutations are idempotent: calling `initiate_payment` twice with
/// the same `order_id` returns the existing order without creating a
/// duplicate. This is critical because network retries are expected.
pub trait BridgeOperator: Send + Sync {
    /// Initiate a payment to a mobile money phone number.
    ///
    /// # Arguments
    ///
    /// * `order` — The payment order to execute
    ///
    /// # Returns
    ///
    /// The updated payment order with provider reference and status.
    fn initiate_payment(&mut self, order: BridgePaymentOrder) -> Result<BridgePaymentOrder, BridgeError>;

    /// Query the current status of a payment order.
    ///
    /// This may involve calling the provider's API to get the latest
    /// status, or returning the cached status if the provider has
    /// already sent a callback.
    fn query_status(&self, order_id: &str) -> Result<BridgePaymentStatus, BridgeError>;

    /// Process a callback from the mobile money provider.
    ///
    /// Providers send callbacks when a payment is confirmed or fails.
    /// The callback payload is verified (HMAC) and the order status
    /// is updated accordingly.
    ///
    /// # Arguments
    ///
    /// * `order_id` — The order ID from the callback
    /// * `status` — The new status reported by the provider
    /// * `provider_ref` — The provider's transaction reference
    /// * `hmac_signature` — The HMAC-SHA256 signature for verification
    /// * `payload` — The raw callback payload
    fn process_callback(
        &mut self,
        order_id: &str,
        status: BridgePaymentStatus,
        provider_ref: Option<&str>,
        hmac_signature: &[u8],
        payload: &[u8],
    ) -> Result<BridgePaymentOrder, BridgeError>;

    /// Refund a failed payment.
    ///
    /// Returns OMNIA tokens to the sender's account. Only orders in
    /// `Failed` or `Reversing` status can be refunded.
    fn refund(&mut self, order_id: &str) -> Result<BridgePaymentOrder, BridgeError>;

    /// Check if the bridge operator (and its provider) is healthy.
    ///
    /// Returns `Ok(())` if the provider API is reachable and
    /// the circuit breaker is closed.
    fn health_check(&self) -> Result<(), BridgeError>;

    /// Get the provider this operator handles.
    fn provider(&self) -> MobileProvider;

    /// Get the circuit breaker for this operator.
    fn circuit_breaker(&self) -> &CircuitBreaker;

    /// Get a mutable reference to the circuit breaker.
    fn circuit_breaker_mut(&mut self) -> &mut CircuitBreaker;
}

// ------------------------------------------------------------------
// Mock provider adapter (for testing)
// ------------------------------------------------------------------

/// A mock bridge operator for testing.
///
/// Simulates provider responses without making real API calls.
/// All payments succeed immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockBridgeOperator {
    /// The provider this mock handles.
    provider: MobileProvider,
    /// In-memory order store.
    orders: BTreeMap<String, BridgePaymentOrder>,
    /// Circuit breaker.
    circuit_breaker: CircuitBreaker,
    /// Whether to simulate failures.
    simulate_failures: bool,
}

impl MockBridgeOperator {
    /// Create a new mock bridge operator for the given provider.
    pub fn new(provider: MobileProvider) -> Self {
        Self {
            provider,
            orders: BTreeMap::new(),
            circuit_breaker: CircuitBreaker::default(),
            simulate_failures: false,
        }
    }

    /// Create a mock that simulates all payments as failures.
    pub fn new_failing(provider: MobileProvider) -> Self {
        Self {
            provider,
            orders: BTreeMap::new(),
            circuit_breaker: CircuitBreaker::new(3, 1, 1000),
            simulate_failures: true,
        }
    }
}

impl BridgeOperator for MockBridgeOperator {
    fn initiate_payment(&mut self, mut order: BridgePaymentOrder) -> Result<BridgePaymentOrder, BridgeError> {
        // Check circuit breaker
        let now_ms = order.created_at;
        if !self.circuit_breaker.allow_request(now_ms) {
            return Err(BridgeError::CircuitOpen);
        }

        // Idempotency: return existing order if present
        if let Some(existing) = self.orders.get(&order.order_id) {
            return Ok(existing.clone());
        }

        if self.simulate_failures {
            order.status = BridgePaymentStatus::Failed;
            order.provider_ref = Some(format!("mock-fail-{}", order.order_id));
            self.orders.insert(order.order_id.clone(), order.clone());
            self.circuit_breaker.record_failure(now_ms);
            return Ok(order);
        }

        // Validate phone number (basic E.164 check)
        if !order.phone_number.starts_with('+') || order.phone_number.len() < 12 {
            self.circuit_breaker.record_failure(now_ms);
            return Err(BridgeError::InvalidPhoneNumber(order.phone_number.clone()));
        }

        // Validate amount
        if order.amount_ghs_pesewas < 100 {
            // minimum 1 GHS = 100 pesewas
            return Err(BridgeError::AmountBelowMinimum {
                min_pesewas: 100,
                requested: order.amount_ghs_pesewas,
            });
        }

        order.status = BridgePaymentStatus::Confirmed;
        order.provider_ref = Some(format!("mock-ref-{}", order.order_id));
        self.orders.insert(order.order_id.clone(), order.clone());
        self.circuit_breaker.record_success();
        Ok(order)
    }

    fn query_status(&self, order_id: &str) -> Result<BridgePaymentStatus, BridgeError> {
        self.orders
            .get(order_id)
            .map(|o| o.status)
            .ok_or_else(|| BridgeError::OrderNotFound(order_id.to_string()))
    }

    fn process_callback(
        &mut self,
        order_id: &str,
        status: BridgePaymentStatus,
        provider_ref: Option<&str>,
        _hmac_signature: &[u8],
        _payload: &[u8],
    ) -> Result<BridgePaymentOrder, BridgeError> {
        let order = self
            .orders
            .get_mut(order_id)
            .ok_or_else(|| BridgeError::OrderNotFound(order_id.to_string()))?;
        order.status = status;
        if let Some(ref_val) = provider_ref {
            order.provider_ref = Some(ref_val.to_string());
        }
        Ok(order.clone())
    }

    fn refund(&mut self, order_id: &str) -> Result<BridgePaymentOrder, BridgeError> {
        let order = self
            .orders
            .get_mut(order_id)
            .ok_or_else(|| BridgeError::OrderNotFound(order_id.to_string()))?;
        if order.status != BridgePaymentStatus::Failed && order.status != BridgePaymentStatus::Reversing {
            return Err(BridgeError::ProviderError {
                provider: self.provider,
                code: "INVALID_STATE".to_string(),
                message: format!("Cannot refund order in {:?} status", order.status),
            });
        }
        order.status = BridgePaymentStatus::Refunded;
        Ok(order.clone())
    }

    fn health_check(&self) -> Result<(), BridgeError> {
        if self.circuit_breaker.state == CircuitState::Open {
            Err(BridgeError::CircuitOpen)
        } else {
            Ok(())
        }
    }

    fn provider(&self) -> MobileProvider {
        self.provider
    }

    fn circuit_breaker(&self) -> &CircuitBreaker {
        &self.circuit_breaker
    }

    fn circuit_breaker_mut(&mut self) -> &mut CircuitBreaker {
        &mut self.circuit_breaker
    }
}

// ------------------------------------------------------------------
// BridgeRegistry — manages multiple providers
// ------------------------------------------------------------------

/// Registry of bridge operators, indexed by provider.
///
/// Provides a single entry point for the node to route bridge
/// operations to the correct provider adapter. Currently uses
/// concrete `MockBridgeOperator` instances for serializability.
/// Production implementations will add provider-specific variants.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BridgeRegistry {
    /// Registered mock bridge operators, keyed by provider name.
    mock_operators: BTreeMap<String, MockBridgeOperator>,
}

impl BridgeRegistry {
    /// Create a new empty bridge registry.
    pub fn new() -> Self {
        Self {
            mock_operators: BTreeMap::new(),
        }
    }

    /// Register a mock bridge operator for a provider.
    pub fn register(&mut self, operator: MockBridgeOperator) {
        let key = format!("{:?}", operator.provider());
        self.mock_operators.insert(key, operator);
    }

    /// Get a reference to a mock bridge operator for a specific provider.
    pub fn get_mock(&self, provider: MobileProvider) -> Option<&MockBridgeOperator> {
        let key = format!("{:?}", provider);
        self.mock_operators.get(&key)
    }

    /// Get a mutable reference to a mock bridge operator.
    pub fn get_mock_mut(&mut self, provider: MobileProvider) -> Option<&mut MockBridgeOperator> {
        let key = format!("{:?}", provider);
        self.mock_operators.get_mut(&key)
    }

    /// List all registered providers.
    pub fn providers(&self) -> Vec<MobileProvider> {
        self.mock_operators.values().map(|op| op.provider()).collect()
    }

    /// Check health of all registered operators.
    pub fn health_check_all(&self) -> BTreeMap<MobileProvider, Result<(), BridgeError>> {
        let mut results = BTreeMap::new();
        for op in self.mock_operators.values() {
            results.insert(op.provider(), op.health_check());
        }
        results
    }
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn make_order(phone: &str, amount_pesewas: u64) -> BridgePaymentOrder {
        BridgePaymentOrder {
            order_id: uuid_string(),
            provider: MobileProvider::Mtn,
            phone_number: phone.to_string(),
            amount_ghs_pesewas: amount_pesewas,
            amount_omnia: amount_pesewas / 10,
            status: BridgePaymentStatus::Pending,
            created_at: 1000,
            updated_at: 1000,
            provider_ref: None,
            from_account: [0u8; 32],
        }
    }

    fn uuid_string() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        format!("order-{:03}", COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    // MobileProvider tests

    #[test]
    fn test_provider_ussd_codes() {
        assert_eq!(MobileProvider::Mtn.ussd_code(), "*170#");
        assert_eq!(MobileProvider::Telecel.ussd_code(), "*110#");
        assert_eq!(MobileProvider::At.ussd_code(), "*505#");
    }

    #[test]
    fn test_provider_from_str_loose() {
        assert_eq!(MobileProvider::from_str_loose("mtn"), Some(MobileProvider::Mtn));
        assert_eq!(MobileProvider::from_str_loose("MTN"), Some(MobileProvider::Mtn));
        assert_eq!(MobileProvider::from_str_loose("Telecel"), Some(MobileProvider::Telecel));
        assert_eq!(
            MobileProvider::from_str_loose("vodafone"),
            Some(MobileProvider::Telecel)
        );
        assert_eq!(MobileProvider::from_str_loose("at"), Some(MobileProvider::At));
        assert_eq!(MobileProvider::from_str_loose("airteltigo"), Some(MobileProvider::At));
        assert_eq!(MobileProvider::from_str_loose("unknown"), None);
    }

    // CircuitBreaker tests

    #[test]
    fn test_circuit_breaker_starts_closed() {
        let mut cb = CircuitBreaker::default();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request(0));
    }

    #[test]
    fn test_circuit_breaker_opens_after_threshold() {
        let mut cb = CircuitBreaker::new(3, 1, 1000);
        cb.record_failure(0);
        cb.record_failure(0);
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure(0);
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request(0));
    }

    #[test]
    fn test_circuit_breaker_half_open_after_timeout() {
        let mut cb = CircuitBreaker::new(3, 1, 1000);
        for _ in 0..3 {
            cb.record_failure(0);
        }
        assert_eq!(cb.state(), CircuitState::Open);
        // Before timeout
        assert!(!cb.allow_request(999));
        // After timeout
        assert!(cb.allow_request(1001));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_circuit_breaker_closes_on_success_in_half_open() {
        let mut cb = CircuitBreaker::new(3, 1, 1000);
        for _ in 0..3 {
            cb.record_failure(0);
        }
        cb.allow_request(1001); // transition to half-open
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_reopens_on_failure_in_half_open() {
        let mut cb = CircuitBreaker::new(3, 1, 1000);
        for _ in 0..3 {
            cb.record_failure(0);
        }
        cb.allow_request(1001); // transition to half-open
        cb.record_failure(1001);
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let mut cb = CircuitBreaker::new(3, 1, 1000);
        for _ in 0..5 {
            cb.record_failure(0);
        }
        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.consecutive_failures, 0);
    }

    #[test]
    fn test_circuit_breaker_success_resets_failure_count() {
        let mut cb = CircuitBreaker::new(5, 1, 1000);
        cb.record_failure(0);
        cb.record_failure(0);
        cb.record_success();
        assert_eq!(cb.consecutive_failures, 0);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    // MockBridgeOperator tests

    #[test]
    fn test_mock_initiate_payment_success() {
        let mut op = MockBridgeOperator::new(MobileProvider::Mtn);
        let order = make_order("+233201234567", 5000); // 50 GHS
        let result = op.initiate_payment(order).unwrap();
        assert_eq!(result.status, BridgePaymentStatus::Confirmed);
        assert!(result.provider_ref.is_some());
    }

    #[test]
    fn test_mock_initiate_payment_invalid_phone() {
        let mut op = MockBridgeOperator::new(MobileProvider::Mtn);
        let order = make_order("0201234567", 5000); // missing +
        let result = op.initiate_payment(order);
        assert!(matches!(result, Err(BridgeError::InvalidPhoneNumber(_))));
    }

    #[test]
    fn test_mock_initiate_payment_below_minimum() {
        let mut op = MockBridgeOperator::new(MobileProvider::Mtn);
        let order = make_order("+233201234567", 50); // 0.50 GHS — below 1 GHS minimum
        let result = op.initiate_payment(order);
        assert!(matches!(result, Err(BridgeError::AmountBelowMinimum { .. })));
    }

    #[test]
    fn test_mock_idempotency() {
        let mut op = MockBridgeOperator::new(MobileProvider::Mtn);
        let mut order = make_order("+233201234567", 5000);
        // Use a fixed ID for this test
        order.order_id = "idem-test".to_string();
        let first = op.initiate_payment(order.clone()).unwrap();
        // Resubmit with same order_id
        let second = op.initiate_payment(order).unwrap();
        assert_eq!(first.order_id, second.order_id);
        assert_eq!(first.provider_ref, second.provider_ref);
    }

    #[test]
    fn test_mock_query_status() {
        let mut op = MockBridgeOperator::new(MobileProvider::Mtn);
        let order = make_order("+233201234567", 5000);
        let order_id = order.order_id.clone();
        op.initiate_payment(order).unwrap();
        let status = op.query_status(&order_id).unwrap();
        assert_eq!(status, BridgePaymentStatus::Confirmed);
    }

    #[test]
    fn test_mock_query_status_not_found() {
        let op = MockBridgeOperator::new(MobileProvider::Mtn);
        let result = op.query_status("nonexistent");
        assert!(matches!(result, Err(BridgeError::OrderNotFound(_))));
    }

    #[test]
    fn test_mock_refund_success() {
        let mut op = MockBridgeOperator::new_failing(MobileProvider::Mtn);
        let order = make_order("+233201234567", 5000);
        let order_id = order.order_id.clone();
        op.initiate_payment(order).unwrap(); // will fail
        let refunded = op.refund(&order_id).unwrap();
        assert_eq!(refunded.status, BridgePaymentStatus::Refunded);
    }

    #[test]
    fn test_mock_refund_invalid_state() {
        let mut op = MockBridgeOperator::new(MobileProvider::Mtn);
        let order = make_order("+233201234567", 5000);
        let order_id = order.order_id.clone();
        op.initiate_payment(order).unwrap(); // succeeds
        let result = op.refund(&order_id);
        assert!(result.is_err()); // can't refund confirmed order
    }

    #[test]
    fn test_mock_health_check() {
        let op = MockBridgeOperator::new(MobileProvider::Mtn);
        assert!(op.health_check().is_ok());
    }

    #[test]
    fn test_mock_circuit_breaker_opens() {
        let mut op = MockBridgeOperator::new_failing(MobileProvider::Mtn);
        // Trigger 3 failures to open circuit (reset_timeout=1000ms)
        for i in 0..3 {
            let mut order = make_order("+233201234567", 5000);
            order.created_at = i as u64;
            let _ = op.initiate_payment(order);
        }
        // Circuit opened at created_at=2, timeout=1000ms.
        // At created_at=1002 the timeout has NOT elapsed (1002-2=1000, need >= 1000).
        // At created_at=1003 the timeout HAS elapsed (1003-2=1001 >= 1000) -> half-open.
        // Use created_at=3 (just after open, well within timeout) to get CircuitOpen.
        let mut order = make_order("+233201234567", 5000);
        order.created_at = 3;
        let result = op.initiate_payment(order);
        assert!(matches!(result, Err(BridgeError::CircuitOpen)));
    }

    // BridgeRegistry tests

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = BridgeRegistry::new();
        let mtn = MockBridgeOperator::new(MobileProvider::Mtn);
        let telecel = MockBridgeOperator::new(MobileProvider::Telecel);
        registry.register(mtn);
        registry.register(telecel);

        assert!(registry.get_mock(MobileProvider::Mtn).is_some());
        assert!(registry.get_mock(MobileProvider::Telecel).is_some());
        assert!(registry.get_mock(MobileProvider::At).is_none());
    }

    #[test]
    fn test_registry_providers_list() {
        let mut registry = BridgeRegistry::new();
        registry.register(MockBridgeOperator::new(MobileProvider::Mtn));
        registry.register(MockBridgeOperator::new(MobileProvider::At));
        let providers = registry.providers();
        assert!(providers.contains(&MobileProvider::Mtn));
        assert!(providers.contains(&MobileProvider::At));
        assert_eq!(providers.len(), 2);
    }

    #[test]
    fn test_registry_health_check_all() {
        let mut registry = BridgeRegistry::new();
        registry.register(MockBridgeOperator::new(MobileProvider::Mtn));
        let results = registry.health_check_all();
        assert!(results.get(&MobileProvider::Mtn).unwrap().is_ok());
    }

    // Serialization tests

    #[test]
    fn test_order_serialization() {
        let order = make_order("+233201234567", 5000);
        let bytes = postcard::to_allocvec(&order).unwrap();
        let restored: BridgePaymentOrder = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(restored.order_id, order.order_id);
        assert_eq!(restored.phone_number, order.phone_number);
        assert_eq!(restored.amount_ghs_pesewas, order.amount_ghs_pesewas);
    }

    #[test]
    fn test_circuit_breaker_serialization() {
        let mut cb = CircuitBreaker::new(5, 2, 10000);
        cb.record_failure(0);
        cb.record_failure(0);
        let bytes = postcard::to_allocvec(&cb).unwrap();
        let restored: CircuitBreaker = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(restored.consecutive_failures, 2);
    }
}
