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
    // Special - traded via auction but not stockpiled
    Labor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum GoodFamily {
    BulkStaples,
    ProcessedMaterials,
    FinishedGoods,
    CapitalGoods,
}

/// Family for labor - separate from physical goods
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum GoodKind {
    Physical, // Can be stockpiled and shipped
    Labor,    // Settlement-scoped, consumed immediately
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
            Good::Labor => GoodFamily::BulkStaples, // Doesn't really apply, but need a value
        }
    }

    pub fn kind(&self) -> GoodKind {
        match self {
            Good::Labor => GoodKind::Labor,
            _ => GoodKind::Physical,
        }
    }

    /// Returns an iterator over all goods (including Labor)
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
            Good::Labor,
        ]
        .into_iter()
    }

    /// Returns an iterator over physical goods only (excludes Labor)
    pub fn physical() -> impl Iterator<Item = Good> {
        Self::all().filter(|g| g.kind() == GoodKind::Physical)
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
    // Primary (extraction)
    Farm,
    Fishery,
    LumberCamp,
    Mine,
    Pasture,
    // Processing
    Mill,
    Foundry,
    Weaver,
    // Finished
    Bakery,
    Toolsmith,
    // Capital
    Shipyard,
    // Special - owned by Settlement Org, fallback for unassigned workers
    SubsistenceFarm,
}

// ============================================================================
// Organization Type
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum OrgType {
    Regular,    // Player or AI trading company
    Settlement, // Special: owns subsistence farm, hardcoded behavior
}

// ============================================================================
// Location ID - Where stockpiles exist
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum LocationId {
    Settlement(u64), // SettlementId as u64 for WASM
    Ship(u64),       // ShipId as u64 for WASM
}

impl LocationId {
    pub fn from_settlement(id: SettlementId) -> Self {
        LocationId::Settlement(id.to_u64())
    }

    pub fn from_ship(id: ShipId) -> Self {
        LocationId::Ship(id.to_u64())
    }
}

// ============================================================================
// Entity ID - Who participates in auctions
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum EntityId {
    Org(u64),        // OrgId as u64 for WASM
    Population(u64), // SettlementId as u64 (population of that settlement)
    Ship(u64),       // ShipId as u64 - trades from cargo, money to/from owner
}

impl EntityId {
    pub fn from_org(id: OrgId) -> Self {
        EntityId::Org(id.to_u64())
    }

    pub fn from_population(settlement_id: SettlementId) -> Self {
        EntityId::Population(settlement_id.to_u64())
    }

    pub fn from_ship(ship_id: ShipId) -> Self {
        EntityId::Ship(ship_id.to_u64())
    }
}
