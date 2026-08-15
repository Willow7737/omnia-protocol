//! Error types for the asset registry.

/// Errors that can occur during asset registry operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// The requested asset was not found in the registry.
    #[error("asset {0} not found")]
    AssetNotFound(u32),

    /// An asset with this ID already exists.
    #[error("asset {0} already exists (symbol: {1})")]
    AssetAlreadyExists(u32, String),

    /// The asset is frozen and cannot be used for the requested operation.
    #[error("asset {0} is frozen")]
    AssetFrozen(u32),

    /// The caller is not authorized to perform this registration.
    #[error("unauthorized asset registration")]
    UnauthorizedRegistration,

    /// The asset's decimals are invalid (must be 0–255).
    #[error("invalid decimals: {0}")]
    InvalidDecimals(u8),

    /// The symbol is empty or too long.
    #[error("invalid symbol: {0}")]
    InvalidSymbol(String),

    /// The display name is empty or too long.
    #[error("invalid display name: {0}")]
    InvalidDisplayName(String),

    /// A Spec §4.4 invariant would be violated.
    #[error("invariant violation: {0}")]
    InvariantViolation(String),

    /// Minting would exceed the asset's hard cap.
    #[error("mint would exceed hard cap for asset {0}: current {1} + requested {2} > max {3}")]
    SupplyExceedsHardCap(u32, u64, u64, u64),

    /// Burn amount exceeds available supply.
    #[error("burn would exceed supply for asset {0}: current {1} < requested {2}")]
    InsufficientSupply(u32, u64, u64),

    /// Attempted to transfer a non-transferable asset.
    #[error("asset {0} is non-transferable ({1})")]
    NonTransferable(u32, String),

    /// Attempted to burn UBC (prohibited per Spec §7.3).
    #[error("UBC must not be burned as OMNIA (Spec §7.3)")]
    UbcBurnProhibited,

    /// Attempted to mint OMNIA from an external adapter (Spec §4.4).
    #[error("external adapter cannot mint OMNIA (Spec §4.4)")]
    ExternalMintOfOmniaProhibited,

    /// Supply accounting error.
    #[error("supply accounting error: {0}")]
    SupplyAccounting(String),

    /// Treasury allocation would exceed a hard limit (Spec §6.2, §15).
    #[error("treasury limit exceeded ({limit_type}): requested {requested}, allowed {allowed}")]
    TreasuryLimitExceeded {
        /// Type of limit that was exceeded.
        limit_type: String,
        /// Amount requested.
        requested: u64,
        /// Maximum allowed.
        allowed: u64,
    },

    /// Treasury is paused (circuit breaker tripped).
    #[error("treasury paused: {0}")]
    TreasuryPaused(String),

    /// Wallet not in approved treasury wallet list.
    #[error("wallet {0} not authorized for treasury operations")]
    UnauthorizedTreasuryWallet(String),

    /// Insufficient balance for an asset-scoped transfer.
    #[error("insufficient balance for asset {0}: have {1}, need {2}")]
    InsufficientBalance(u32, u64, u64),
}
