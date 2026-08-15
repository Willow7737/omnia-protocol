//! Core asset types — Financial Specification §4.1, §4.2

use serde::{Deserialize, Serialize};

/// Unique identifier for an asset type.
///
/// Well-known IDs (allocated at genesis):
/// - `0` → OMNIA (native transferable economic asset)
/// - `1` → UBC (non-transferable participation allowance)
/// - `2+` → External assets (BTC, etc.) registered via governance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct AssetId(pub u32);

impl AssetId {
    /// OMNIA — native transferable economic asset (Spec §3.2, §4.2).
    pub const OMNIA: Self = Self(0);
    /// UBC — non-transferable participation allowance (Spec §3.1, §4.2).
    pub const UBC: Self = Self(1);

    /// Create a new `AssetId` for an external or future asset.
    /// Callers must ensure uniqueness via the registry.
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Return the raw u32 value.
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AssetId({})", self.0)
    }
}

/// Asset classification per Spec §4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetClass {
    /// Non-transferable participation and compute allowance (e.g., UBC).
    ParticipationAllowance,
    /// Native transferable economic asset (e.g., OMNIA).
    NativeEconomicAsset,
    /// External settlement asset (e.g., BTC).
    ExternalSettlementAsset,
    /// Future payment unit — requires separate legal review.
    FuturePaymentUnit,
}

/// Whether an asset can be transferred between accounts.
/// Per Spec §4.2, UBC is non-transferable; OMNIA is transferable;
/// external assets depend on custody model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transferability {
    /// Fully transferable between any accounts.
    Transferable,
    /// Non-transferable — soulbound to the account (e.g., UBC).
    NonTransferable,
    /// Transfer restricted to specific conditions or counterparties.
    Restricted,
}

/// Who is authorized to mint this asset.
/// Per Spec §6.1, issuance authority is divided into distinct roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MintPolicy {
    /// No minting is permitted (fixed supply).
    Fixed,
    /// Only the genesis authority can mint (one-time allocation).
    GenesisOnly,
    /// Bounded treasury or governance policy controls minting.
    Bounded { max_supply: u64 },
    /// External chain or qualified partner controls issuance.
    External,
    /// Epoch/eligibility protocol controls issuance (e.g., UBC reset).
    EpochEligibility,
}

/// Burn policy for an asset.
/// Per Spec §7.3, burn policy must be defined per asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BurnPolicy {
    /// Burning is not permitted for this asset.
    Prohibited,
    /// Anyone can burn their own holdings.
    OwnerInitiated,
    /// Only authorized roles can burn (e.g., fee burn, slashing).
    AuthorizedOnly,
}

/// Fee policy for an asset.
/// Per Spec §7.1, UBC and OMNIA fees must remain distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeePolicy {
    /// This asset cannot be used to pay fees.
    NotAccepted,
    /// This asset is accepted for base fees.
    BaseFee,
    /// This asset is accepted for priority fees only.
    PriorityFeeOnly,
    /// Accepted for both base and priority fees.
    BaseAndPriority,
}

/// Chain scope — which chain(s) this asset exists on.
/// Per Spec §3.3, external assets have their own chain identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainScope {
    /// Native to the Omnia chain (OMNIA, UBC).
    Native,
    /// External chain with a network identifier (e.g., "bitcoin-mainnet").
    External { chain_id: String },
    /// Bridged — exists on Omnia as a representation of an external asset.
    Bridged { source_chain: String },
}

/// Operational status of an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetStatus {
    /// Asset is active and fully operational.
    Active,
    /// Asset is frozen — no transfers, mints, or burns permitted.
    Frozen,
    /// Asset is being deprecated (existing balances honored, no new mints).
    Deprecated,
    /// Asset is a placeholder or stub (e.g., adapter stub exists but is
    /// not production-ready). Per Spec §13, no asset should be marketed as
    /// supported merely because a stub exists.
    Stub,
}

/// Full asset definition per Spec §4.1.
///
/// The actual Rust representation may differ structurally but MUST
/// cover all these semantics. This is the canonical definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetDefinition {
    /// Unique identifier for this asset.
    pub asset_id: AssetId,
    /// Ticker symbol (e.g., "OMNIA", "UBC", "BTC").
    pub symbol: String,
    /// Human-readable display name (e.g., "Omnia Token").
    pub display_name: String,
    /// Decimal precision (9 for OMNIA, 8 for BTC).
    pub decimals: u8,
    /// Asset classification per Spec §4.2.
    pub asset_class: AssetClass,
    /// Whether and how this asset can be transferred.
    pub transferability: Transferability,
    /// Who can issue/mint this asset.
    pub mint_policy: MintPolicy,
    /// Burn policy for this asset.
    pub burn_policy: BurnPolicy,
    /// Fee policy — whether this asset can be used to pay fees.
    pub fee_policy: FeePolicy,
    /// Chain scope — which chain this asset lives on.
    pub chain_scope: ChainScope,
    /// Current operational status.
    pub status: AssetStatus,
    /// Existential deposit — minimum balance to keep an account alive.
    /// For UBC this is 0; for OMNIA this is 1_000_000_000_000 (1 token).
    pub existential_deposit: u64,
}

impl AssetDefinition {
    /// Convenience: create the OMNIA asset definition per Spec §3.2, §5.1.
    /// Uses the working hard cap of 1,000,000,000 OMNIA (10^9 * 10^12 = 10^21 plancks).
    pub fn omnia() -> Self {
        Self {
            asset_id: AssetId::OMNIA,
            symbol: "OMNIA".into(),
            display_name: "Omnia Token".into(),
            decimals: 9,
            asset_class: AssetClass::NativeEconomicAsset,
            transferability: Transferability::Transferable,
            mint_policy: MintPolicy::Bounded {
                max_supply: 1_000_000_000_000_000_000, // 1B * 10^9
            },
            burn_policy: BurnPolicy::AuthorizedOnly,
            fee_policy: FeePolicy::BaseAndPriority,
            chain_scope: ChainScope::Native,
            status: AssetStatus::Active,
            existential_deposit: 1_000_000_000, // 1 OMNIA
        }
    }

    /// Convenience: create the UBC asset definition per Spec §3.1.
    pub fn ubc() -> Self {
        Self {
            asset_id: AssetId::UBC,
            symbol: "UBC".into(),
            display_name: "Utility Balance Credit".into(),
            decimals: 9,
            asset_class: AssetClass::ParticipationAllowance,
            transferability: Transferability::NonTransferable,
            mint_policy: MintPolicy::EpochEligibility,
            burn_policy: BurnPolicy::Prohibited, // UBC MUST NOT be burned as OMNIA (Spec §7.3)
            fee_policy: FeePolicy::NotAccepted,  // UBC is for compute, not fees
            chain_scope: ChainScope::Native,
            status: AssetStatus::Active,
            existential_deposit: 0,
        }
    }

    /// Return true if this asset is the UBC participation allowance.
    /// Used to enforce the Spec §4.4 invariant: no UBC operation can create
    /// a transferable OMNIA balance.
    pub fn is_ubc(&self) -> bool {
        self.asset_id == AssetId::UBC
    }

    /// Return true if this asset is the native OMNIA token.
    pub fn is_omnia(&self) -> bool {
        self.asset_id == AssetId::OMNIA
    }

    /// Return true if transfers are allowed for this asset.
    pub fn is_transferable(&self) -> bool {
        self.status == AssetStatus::Active && matches!(self.transferability, Transferability::Transferable)
    }

    /// Return the max supply if bounded, None if unbounded or fixed-at-current.
    pub fn max_supply(&self) -> Option<u64> {
        match self.mint_policy {
            MintPolicy::Bounded { max_supply } => Some(max_supply),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omnia_definition_matches_spec() {
        let omnia = AssetDefinition::omnia();
        assert_eq!(omnia.asset_id, AssetId::OMNIA);
        assert_eq!(omnia.symbol, "OMNIA");
        assert_eq!(omnia.decimals, 9);
        assert_eq!(omnia.asset_class, AssetClass::NativeEconomicAsset);
        assert_eq!(omnia.transferability, Transferability::Transferable);
        assert_eq!(omnia.status, AssetStatus::Active);
        assert!(omnia.is_omnia());
        assert!(!omnia.is_ubc());
        assert!(omnia.is_transferable());
        assert_eq!(omnia.max_supply(), Some(1_000_000_000_000_000_000u64));
    }

    #[test]
    fn ubc_definition_matches_spec() {
        let ubc = AssetDefinition::ubc();
        assert_eq!(ubc.asset_id, AssetId::UBC);
        assert_eq!(ubc.symbol, "UBC");
        assert_eq!(ubc.decimals, 9);
        assert_eq!(ubc.asset_class, AssetClass::ParticipationAllowance);
        assert_eq!(ubc.transferability, Transferability::NonTransferable);
        assert!(!ubc.is_transferable());
        assert!(ubc.is_ubc());
        assert!(!ubc.is_omnia());
        // UBC burn MUST be prohibited (Spec §7.3)
        assert_eq!(ubc.burn_policy, BurnPolicy::Prohibited);
        // UBC MUST NOT be used for fees
        assert_eq!(ubc.fee_policy, FeePolicy::NotAccepted);
    }

    #[test]
    fn ubc_cannot_become_omnia() {
        let ubc = AssetDefinition::ubc();
        let omnia = AssetDefinition::omnia();
        // AssetIds are distinct — no operation can conflate them
        assert_ne!(ubc.asset_id, omnia.asset_id);
        // UBC is non-transferable, OMNIA is transferable
        assert_ne!(ubc.transferability, omnia.transferability);
        // UBC burn is prohibited, OMNIA burn is authorized-only
        assert_ne!(ubc.burn_policy, omnia.burn_policy);
    }

    #[test]
    fn frozen_asset_not_transferable() {
        let mut omnia = AssetDefinition::omnia();
        assert!(omnia.is_transferable());
        omnia.status = AssetStatus::Frozen;
        assert!(!omnia.is_transferable());
    }
}
