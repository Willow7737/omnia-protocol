//! Error types for fee and burn operations.

/// Errors that can occur during fee/burn operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FeeError {
    /// The burn ratio exceeds the initial maximum (500 bps = 5%).
    #[error("burn ratio {requested_bps}bps exceeds initial max {max_bps}bps")]
    BurnRatioExceedsInitial {
        /// Requested basis points.
        requested_bps: u16,
        /// Maximum allowed basis points.
        max_bps: u16,
    },

    /// The burn ratio exceeds the absolute governance ceiling (2500 bps = 25%).
    #[error("burn ratio {requested_bps}bps exceeds absolute ceiling {ceiling_bps}bps")]
    BurnRatioExceedsCeiling {
        /// Requested basis points.
        requested_bps: u16,
        /// Ceiling basis points.
        ceiling_bps: u16,
    },

    /// Priority fee exceeds the maximum allowed.
    #[error("priority fee {requested} exceeds maximum {maximum}")]
    PriorityFeeExceeded {
        /// Requested priority fee.
        requested: u64,
        /// Maximum allowed.
        maximum: u64,
    },

    /// Attempted to burn UBC (prohibited per Spec §7.3).
    #[error("UBC must not be burned as OMNIA (Spec §7.3)")]
    UbcBurnProhibited,

    /// Attempted to represent external-chain fee as OMNIA burn.
    #[error("external-chain fee must not be misrepresented as OMNIA burn (Spec §7.3)")]
    ExternalFeeMisrepresentedAsBurn,

    /// Burns are currently paused.
    #[error("burns are paused: {0}")]
    BurnsPaused(String),

    /// Attempted to pause when already paused.
    #[error("burns are already paused")]
    BurnAlreadyPaused,

    /// Attempted to resume when not paused.
    #[error("burns are not currently paused")]
    BurnNotPaused,

    /// Attempted to change burn ratio while paused.
    #[error("cannot change burn ratio while burns are paused")]
    BurnPausedWhileChanging,

    /// Arithmetic overflow during fee or burn calculation.
    #[error("arithmetic overflow in {0}")]
    ArithmeticOverflow(String),

    /// The fee schedule is misconfigured.
    #[error("fee schedule misconfiguration: {0}")]
    FeeScheduleError(String),
}
