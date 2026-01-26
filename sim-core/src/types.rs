use serde::{Deserialize, Serialize};
use slotmap::new_key_type;
use tsify_next::Tsify;

// ============================================================================
// IDs - Using slotmap for generational indices
// ============================================================================

new_key_type! {
    pub struct SettlementId;
    pub struct ShipId;
    pub struct FacilityId;
    pub struct OrgId;
}

/// Trait for converting SlotMap keys to u64 for WASM boundary
pub trait KeyToU64 {
    fn to_u64(self) -> u64;
}

impl KeyToU64 for SettlementId {
    fn to_u64(self) -> u64 {
        self.0.as_ffi()
    }
}

impl KeyToU64 for ShipId {
    fn to_u64(self) -> u64 {
        self.0.as_ffi()
    }
}

impl KeyToU64 for FacilityId {
    fn to_u64(self) -> u64 {
        self.0.as_ffi()
    }
}

impl KeyToU64 for OrgId {
    fn to_u64(self) -> u64 {
        self.0.as_ffi()
    }
}

// ============================================================================
// Goods - The commodities that flow through the economy
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum Good {
    // Primary
    Grain,
    Fish,
    Timber,
    Ore,
    Wool,
    // Processed
    Flour,
    Lumber,
    Iron,
    Cloth,
    // Finished
    Provisions,
    Tools,
    // Capital
    Ships,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum GoodFamily {
    BulkStaples,
    ProcessedMaterials,
    FinishedGoods,
    CapitalGoods,
}

impl Good {
    pub fn family(&self) -> GoodFamily {
        match self {
            Good::Grain | Good::Fish | Good::Timber | Good::Ore | Good::Wool => {
                GoodFamily::BulkStaples
            }
            Good::Flour | Good::Lumber | Good::Iron | Good::Cloth => GoodFamily::ProcessedMaterials,
            Good::Provisions | Good::Tools => GoodFamily::FinishedGoods,
            Good::Ships => GoodFamily::CapitalGoods,
        }
    }

    /// Returns an iterator over all goods
    pub fn all() -> impl Iterator<Item = Good> {
        [
            Good::Grain,
            Good::Fish,
            Good::Timber,
            Good::Ore,
            Good::Wool,
            Good::Flour,
            Good::Lumber,
            Good::Iron,
            Good::Cloth,
            Good::Provisions,
            Good::Tools,
            Good::Ships,
        ]
        .into_iter()
    }
}

// ============================================================================
// Natural Resources - What can be extracted at a location
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum NaturalResource {
    FertileLand, // Enables grain production
    Fishery,     // Enables fishing
    Forest,      // Enables timber
    OreDeposit,  // Enables mining
    Pastureland, // Enables wool/livestock
}

// ============================================================================
// Transport Mode
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum TransportMode {
    Sea,
    River,
    Land,
}

// ============================================================================
// Ship Status
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum ShipStatus {
    InPort,
    EnRoute,
}

// ============================================================================
// Facility Type
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum FacilityType {
    Farm,
    Fishery,
    LumberCamp,
    Mine,
    Pasture,
    Mill,
    Foundry,
    Weaver,
    Bakery,
    Toolsmith,
    Shipyard,
}
