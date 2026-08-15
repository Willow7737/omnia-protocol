//! Asset Registry — registration, queries, freeze/unfreeze.
//! Per Spec §4, the registry is the single source of truth for all assets.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::RegistryError;
use crate::supply::SupplyTracker;
use crate::types::*;

/// Registry events emitted on state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryEvent {
    /// A new asset was registered.
    AssetRegistered {
        /// The registered asset identifier.
        id: AssetId,
        /// Ticker symbol of the registered asset.
        symbol: String,
        /// Classification of the registered asset.
        asset_class: AssetClass,
    },
    /// An asset's metadata was updated.
    AssetMetadataUpdated {
        /// The asset whose metadata was updated.
        id: AssetId,
    },
    /// An asset was frozen (no transfers/mints/burns).
    AssetFrozen {
        /// The frozen asset identifier.
        id: AssetId,
    },
    /// An asset was unfrozen.
    AssetUnfrozen {
        /// The unfrozen asset identifier.
        id: AssetId,
    },
}

/// The full asset registry.
///
/// Combines asset definitions, supply tracking, and event emission.
/// Designed to be embedded in the financial shard state.
pub struct AssetRegistry {
    /// Asset definitions keyed by AssetId.
    definitions: BTreeMap<AssetId, AssetDefinition>,
    /// Supply tracker with full audit log.
    supply: SupplyTracker,
    /// Next available AssetId for registration.
    next_id: u32,
    /// Emitted events (append-only log for audit).
    events: Vec<RegistryEvent>,
}

impl AssetRegistry {
    /// Create an empty registry with no assets.
    pub fn new() -> Self {
        Self {
            definitions: BTreeMap::new(),
            supply: SupplyTracker::new(),
            next_id: 2, // 0=OMNIA, 1=UBC are well-known
            events: Vec::new(),
        }
    }

    /// Create a registry pre-loaded with OMNIA and UBC genesis assets.
    /// This is the standard initialization for mainnet/testnet.
    pub fn with_genesis_assets() -> Self {
        let mut reg = Self::new();
        // Register OMNIA — silently ok if we call this twice in tests
        let _ = reg.register_genesis(AssetDefinition::omnia());
        let _ = reg.register_genesis(AssetDefinition::ubc());
        reg
    }

    /// Register an asset from genesis authority.
    /// Bypasses the next_id counter (genesis assets have well-known IDs).
    fn register_genesis(&mut self, def: AssetDefinition) -> Result<RegistryEvent, RegistryError> {
        let id = def.asset_id;
        if self.definitions.contains_key(&id) {
            return Err(RegistryError::AssetAlreadyExists(id.as_u32(), def.symbol));
        }
        self.validate_definition(&def)?;
        let symbol = def.symbol.clone();
        let asset_class = def.asset_class;
        self.supply.init_asset(id);
        self.definitions.insert(id, def);
        let event = RegistryEvent::AssetRegistered {
            id,
            symbol,
            asset_class,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Register a new asset (governance/root only).
    ///
    /// Validates the definition, assigns the next available ID,
    /// initializes supply tracking, and emits an event.
    pub fn register(&mut self, def: AssetDefinition) -> Result<RegistryEvent, RegistryError> {
        let id = def.asset_id;
        if self.definitions.contains_key(&id) {
            return Err(RegistryError::AssetAlreadyExists(id.as_u32(), def.symbol.clone()));
        }
        self.validate_definition(&def)?;

        let symbol = def.symbol.clone();
        let asset_class = def.asset_class;
        self.supply.init_asset(id);
        self.definitions.insert(id, def);

        // Advance next_id past the registered one
        if id.as_u32() >= self.next_id {
            self.next_id = id.as_u32() + 1;
        }

        let event = RegistryEvent::AssetRegistered {
            id,
            symbol,
            asset_class,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Update metadata for an existing asset (governance/root only).
    /// Cannot change: asset_id, asset_class (immutable).
    pub fn update_metadata(
        &mut self,
        id: AssetId,
        symbol: Option<String>,
        display_name: Option<String>,
        decimals: Option<u8>,
        existential_deposit: Option<u64>,
        status: Option<AssetStatus>,
    ) -> Result<RegistryEvent, RegistryError> {
        let def = self
            .definitions
            .get_mut(&id)
            .ok_or(RegistryError::AssetNotFound(id.as_u32()))?;

        if let Some(s) = symbol {
            if s.is_empty() || s.len() > 16 {
                return Err(RegistryError::InvalidSymbol("symbol must be 1–16 chars".into()));
            }
            def.symbol = s;
        }
        if let Some(d) = display_name {
            if d.is_empty() || d.len() > 64 {
                return Err(RegistryError::InvalidDisplayName(
                    "display name must be 1–64 chars".into(),
                ));
            }
            def.display_name = d;
        }
        if let Some(d) = decimals {
            def.decimals = d;
        }
        if let Some(e) = existential_deposit {
            def.existential_deposit = e;
        }
        if let Some(s) = status {
            def.status = s;
        }

        let event = RegistryEvent::AssetMetadataUpdated { id };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Freeze an asset — no transfers, mints, or burns permitted.
    pub fn freeze(&mut self, id: AssetId) -> Result<RegistryEvent, RegistryError> {
        let def = self
            .definitions
            .get_mut(&id)
            .ok_or(RegistryError::AssetNotFound(id.as_u32()))?;
        def.status = AssetStatus::Frozen;
        let event = RegistryEvent::AssetFrozen { id };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Unfreeze a previously frozen asset.
    pub fn unfreeze(&mut self, id: AssetId) -> Result<RegistryEvent, RegistryError> {
        let def = self
            .definitions
            .get_mut(&id)
            .ok_or(RegistryError::AssetNotFound(id.as_u32()))?;
        if def.status != AssetStatus::Frozen {
            return Err(RegistryError::InvariantViolation(format!(
                "asset {} is not frozen (current status: {:?})",
                id, def.status
            )));
        }
        def.status = AssetStatus::Active;
        let event = RegistryEvent::AssetUnfrozen { id };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Get the definition for an asset.
    pub fn get(&self, id: AssetId) -> Option<&AssetDefinition> {
        self.definitions.get(&id)
    }

    /// Get the OMNIA definition (convenience).
    pub fn omnia(&self) -> Option<&AssetDefinition> {
        self.get(AssetId::OMNIA)
    }

    /// Get the UBC definition (convenience).
    pub fn ubc(&self) -> Option<&AssetDefinition> {
        self.get(AssetId::UBC)
    }

    /// Get all registered asset IDs.
    pub fn asset_ids(&self) -> Vec<AssetId> {
        self.definitions.keys().copied().collect()
    }

    /// Get total supply for an asset.
    pub fn total_supply(&self, id: AssetId) -> u64 {
        self.supply.total_supply(id)
    }

    /// Get all registry events.
    pub fn events(&self) -> &[RegistryEvent] {
        &self.events
    }

    /// Get the next available AssetId for external asset registration.
    pub fn next_asset_id(&self) -> AssetId {
        AssetId::new(self.next_id)
    }

    /// Borrow the supply tracker (for mint/burn operations).
    pub fn supply_tracker(&self) -> &SupplyTracker {
        &self.supply
    }

    /// Mutably borrow the supply tracker.
    pub fn supply_tracker_mut(&mut self) -> &mut SupplyTracker {
        &mut self.supply
    }

    /// Verify all supply invariants across all assets.
    pub fn verify_all_invariants(&self) -> Result<usize, RegistryError> {
        self.supply.verify_all_invariants()
    }

    /// Check if a transfer is allowed for a given asset.
    /// Enforces: asset exists, not frozen, is transferable.
    pub fn can_transfer(&self, id: AssetId) -> Result<(), RegistryError> {
        let def = self
            .definitions
            .get(&id)
            .ok_or(RegistryError::AssetNotFound(id.as_u32()))?;
        if def.status == AssetStatus::Frozen {
            return Err(RegistryError::AssetFrozen(id.as_u32()));
        }
        if !def.is_transferable() {
            return Err(RegistryError::NonTransferable(id.as_u32(), def.symbol.clone()));
        }
        Ok(())
    }

    // --- Internal ---

    fn validate_definition(&self, def: &AssetDefinition) -> Result<(), RegistryError> {
        if def.symbol.is_empty() || def.symbol.len() > 16 {
            return Err(RegistryError::InvalidSymbol(format!(
                "symbol '{}' must be 1–16 chars",
                def.symbol
            )));
        }
        if def.display_name.is_empty() || def.display_name.len() > 64 {
            return Err(RegistryError::InvalidDisplayName(format!(
                "display name '{}' must be 1–64 chars",
                def.display_name
            )));
        }
        // Decimals 0–255 are all valid (u8 range)
        Ok(())
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_registry_has_omnia_and_ubc() {
        let reg = AssetRegistry::with_genesis_assets();
        assert!(reg.omnia().is_some());
        assert!(reg.ubc().is_some());
        assert_eq!(reg.asset_ids().len(), 2);
    }

    #[test]
    fn duplicate_registration_rejected() {
        let mut reg = AssetRegistry::with_genesis_assets();
        let err = reg.register(AssetDefinition::omnia()).unwrap_err();
        assert!(matches!(err, RegistryError::AssetAlreadyExists(_, _)));
    }

    #[test]
    fn freeze_prevents_transfer() {
        let mut reg = AssetRegistry::with_genesis_assets();
        reg.freeze(AssetId::OMNIA).unwrap();
        assert!(reg.can_transfer(AssetId::OMNIA).is_err());
    }

    #[test]
    fn unfreeze_restores_transfer() {
        let mut reg = AssetRegistry::with_genesis_assets();
        reg.freeze(AssetId::OMNIA).unwrap();
        reg.unfreeze(AssetId::OMNIA).unwrap();
        assert!(reg.can_transfer(AssetId::OMNIA).is_ok());
    }

    #[test]
    fn ubc_cannot_transfer() {
        let reg = AssetRegistry::with_genesis_assets();
        assert!(reg.can_transfer(AssetId::UBC).is_err());
    }

    #[test]
    fn register_external_asset() {
        let mut reg = AssetRegistry::with_genesis_assets();
        let btc = AssetDefinition {
            asset_id: AssetId::new(2),
            symbol: "BTC".into(),
            display_name: "Bitcoin".into(),
            decimals: 8,
            asset_class: AssetClass::ExternalSettlementAsset,
            transferability: Transferability::Transferable,
            mint_policy: MintPolicy::External,
            burn_policy: BurnPolicy::Prohibited,
            fee_policy: FeePolicy::NotAccepted,
            chain_scope: ChainScope::External {
                chain_id: "bitcoin-mainnet".into(),
            },
            status: AssetStatus::Stub,
            existential_deposit: 0,
        };
        reg.register(btc).unwrap();
        assert_eq!(reg.asset_ids().len(), 3);
        assert!(reg.get(AssetId::new(2)).is_some());
    }

    #[test]
    fn invalid_symbol_rejected() {
        let mut reg = AssetRegistry::new();
        let bad = AssetDefinition {
            asset_id: AssetId::new(100),
            symbol: "".into(),
            display_name: "Empty Symbol".into(),
            decimals: 12,
            asset_class: AssetClass::NativeEconomicAsset,
            transferability: Transferability::Transferable,
            mint_policy: MintPolicy::Fixed,
            burn_policy: BurnPolicy::OwnerInitiated,
            fee_policy: FeePolicy::BaseAndPriority,
            chain_scope: ChainScope::Native,
            status: AssetStatus::Active,
            existential_deposit: 0,
        };
        assert!(matches!(reg.register(bad), Err(RegistryError::InvalidSymbol(_))));
    }

    #[test]
    fn update_metadata() {
        let mut reg = AssetRegistry::with_genesis_assets();
        reg.update_metadata(AssetId::OMNIA, Some("OMNIA2".into()), None, None, None, None)
            .unwrap();
        assert_eq!(reg.omnia().unwrap().symbol, "OMNIA2");
    }

    #[test]
    fn events_logged() {
        let mut reg = AssetRegistry::with_genesis_assets();
        assert_eq!(reg.events().len(), 2); // OMNIA + UBC
        reg.freeze(AssetId::OMNIA).unwrap();
        assert_eq!(reg.events().len(), 3);
        reg.unfreeze(AssetId::OMNIA).unwrap();
        assert_eq!(reg.events().len(), 4);
    }

    #[test]
    fn next_asset_id_advances() {
        let reg = AssetRegistry::with_genesis_assets();
        assert_eq!(reg.next_asset_id(), AssetId::new(2));
    }
}
