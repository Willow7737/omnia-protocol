//! Error types for the payment order state machine.

/// Errors that can occur during payment order operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaymentError {
    /// Attempted to transition from a terminal state.
    #[error("cannot transition from terminal state {state}: attempted → {attempted}")]
    TerminalState {
        /// The terminal state.
        state: crate::state::PaymentState,
        /// The state that was attempted.
        attempted: crate::state::PaymentState,
    },

    /// The requested state transition is not in the valid transition matrix.
    #[error("invalid transition: {from} → {to}")]
    InvalidTransition {
        /// Current state.
        from: crate::state::PaymentState,
        /// Requested next state.
        to: crate::state::PaymentState,
    },

    /// The order was not found.
    #[error("payment order not found: {0}")]
    OrderNotFound(String),

    /// The order ID already exists.
    #[error("payment order already exists: {0}")]
    OrderAlreadyExists(String),

    /// The quote has expired.
    #[error("quote expired at {expiry_ms}, current time {now_ms}")]
    QuoteExpired {
        /// Quote expiry timestamp (ms).
        expiry_ms: u64,
        /// Current timestamp (ms).
        now_ms: u64,
    },

    /// Payment verification failed.
    #[error("payment verification failed: {0}")]
    VerificationFailed(String),

    /// The GHS amount does not match the quoted amount.
    #[error("amount mismatch: quoted {quoted}, received {received}")]
    AmountMismatch {
        /// Expected amount.
        quoted: u64,
        /// Actual amount received.
        received: u64,
    },

    /// A risk limit was exceeded (Spec §15).
    #[error("risk limit exceeded: {limit_type} (requested {requested}, allowed {allowed})")]
    RiskLimitExceeded {
        /// Type of risk limit.
        limit_type: String,
        /// Amount requested.
        requested: u64,
        /// Maximum allowed.
        allowed: u64,
    },

    /// Circuit breaker is tripped — new allocations paused.
    #[error("circuit breaker tripped: {0}")]
    CircuitBreakerTripped(String),

    /// Treasury inventory insufficient for reservation.
    #[error("insufficient treasury inventory: requested {requested}, available {available}")]
    InsufficientInventory {
        /// Amount requested.
        requested: u64,
        /// Amount available.
        available: u64,
    },

    /// The caller is not authorized for this state transition.
    #[error("unauthorized: {actor} cannot advance from {state} (required: {required})")]
    Unauthorized {
        /// Who tried to perform the transition.
        actor: String,
        /// Current state.
        state: crate::state::PaymentState,
        /// Required authorization.
        required: String,
    },

    /// The payment amount exceeds the per-order GHS limit.
    #[error("per-order GHS limit exceeded: {amount} > {limit}")]
    PerOrderLimitExceeded {
        /// Amount requested.
        amount: u64,
        /// Maximum allowed.
        limit: u64,
    },

    /// The daily customer limit has been exceeded.
    #[error("daily customer limit exceeded for {customer}: {amount} > {limit}")]
    DailyCustomerLimitExceeded {
        /// Customer identifier.
        customer: String,
        /// Amount requested.
        amount: u64,
        /// Maximum allowed.
        limit: u64,
    },

    /// The refund would violate economic delivery invariant (Spec §4.4).
    #[error("refund would leave order economically delivered (state: {0})")]
    RefundAfterDelivery(crate::state::PaymentState),

    /// An invariant violation was detected.
    #[error("invariant violation: {0}")]
    InvariantViolation(String),

    /// Internal overflow during amount calculation.
    #[error("arithmetic overflow: {0}")]
    ArithmeticOverflow(String),
}
