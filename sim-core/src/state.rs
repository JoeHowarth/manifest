use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use tsify_next::Tsify;

use crate::entities::{Facility, Org, Route, Settlement, Ship};
use crate::types::{FacilityId, FacilityType, Good, OrgId, SettlementId, ShipId, ShipStatus};

// ============================================================================
// Recipe - Production formulas for facilities
// ============================================================================

#[derive(Debug, Clone)]
pub struct Recipe {
    pub inputs: Vec<(Good, f32)>, // (good, amount per unit output)
    pub output: Good,
    pub base_output: f32,       // Units produced per tick at full efficiency
    pub optimal_workforce: u32, // Workers needed for full production
}

/// Get the production recipe for a facility type
pub fn get_recipe(facility_type: FacilityType) -> Recipe {
    match facility_type {
        // Primary extraction (no inputs)
        FacilityType::Farm => Recipe {
            inputs: vec![],
            output: Good::Grain,
            base_output: 20.0,
            optimal_workforce: 10,
        },
        FacilityType::Fishery => Recipe {
            inputs: vec![],
            output: Good::Fish,
            base_output: 15.0,
            optimal_workforce: 8,
        },
        FacilityType::LumberCamp => Recipe {
            inputs: vec![],
            output: Good::Lumber, // Produces lumber directly for simplicity
            base_output: 12.0,
            optimal_workforce: 6,
        },
        FacilityType::Mine => Recipe {
            inputs: vec![],
            output: Good::Ore,
            base_output: 10.0,
            optimal_workforce: 8,
        },
        FacilityType::Pasture => Recipe {
            inputs: vec![],
            output: Good::Wool,
            base_output: 8.0,
            optimal_workforce: 4,
        },
        // Processing (transforms inputs)
        FacilityType::Mill => Recipe {
            inputs: vec![(Good::Grain, 1.5)], // 1.5 grain per flour
            output: Good::Flour,
            base_output: 12.0,
            optimal_workforce: 4,
        },
        FacilityType::Foundry => Recipe {
            inputs: vec![(Good::Ore, 2.0)], // 2 ore per iron
            output: Good::Iron,
            base_output: 8.0,
            optimal_workforce: 6,
        },
        FacilityType::Weaver => Recipe {
            inputs: vec![(Good::Wool, 1.2)], // 1.2 wool per cloth
            output: Good::Cloth,
            base_output: 10.0,
            optimal_workforce: 5,
        },
        // Finished goods
        FacilityType::Bakery => Recipe {
            inputs: vec![(Good::Flour, 0.8), (Good::Fish, 0.3)], // Makes provisions
            output: Good::Provisions,
            base_output: 15.0,
            optimal_workforce: 5,
        },
        FacilityType::Toolsmith => Recipe {
            inputs: vec![(Good::Lumber, 0.5), (Good::Iron, 0.5)],
            output: Good::Tools,
            base_output: 6.0,
            optimal_workforce: 4,
        },
        // Capital goods
        FacilityType::Shipyard => Recipe {
            inputs: vec![(Good::Lumber, 5.0), (Good::Iron, 2.0), (Good::Cloth, 1.0)],
            output: Good::Ships,
            base_output: 0.5, // Ships are slow to build
            optimal_workforce: 15,
        },
    }
}

// ============================================================================
// Ship Order - Command to send a ship
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipOrder {
    pub destination: u64,        // SettlementId
    pub cargo: Vec<(Good, f32)>, // Goods to load before departing
}

// ============================================================================
// Game State - The complete simulation state
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub tick: u64,
    pub settlements: SlotMap<SettlementId, Settlement>,
    pub routes: Vec<Route>,
    pub ships: SlotMap<ShipId, Ship>,
    pub facilities: SlotMap<FacilityId, Facility>,
    pub orgs: SlotMap<OrgId, Org>,
}

impl GameState {
    pub fn new() -> Self {
        Self {
            tick: 0,
            settlements: SlotMap::with_key(),
            routes: Vec::new(),
            ships: SlotMap::with_key(),
            facilities: SlotMap::with_key(),
            orgs: SlotMap::with_key(),
        }
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Serializable State Snapshot for JS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct StateSnapshot {
    pub tick: u64,
    pub settlements: Vec<SettlementSnapshot>,
    pub routes: Vec<Route>,
    pub ships: Vec<ShipSnapshot>,
    pub orgs: Vec<OrgSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct FacilitySnapshot {
    pub id: u64,
    pub kind: FacilityType,
    pub owner: u64,
    pub workers: u32,
    pub optimal_workers: u32,
    pub efficiency: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct MarketPriceSnapshot {
    pub good: Good,
    pub price: f32,
    pub available: f32,
    pub last_traded: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SettlementSnapshot {
    pub id: u64,
    pub name: String,
    pub position: (f32, f32),
    pub population: u32,
    pub wealth: f32,
    // Economic data
    pub wage: f32,
    pub labor_demand: f32,
    pub labor_supply: f32,
    pub prices: Vec<MarketPriceSnapshot>,
    pub facilities: Vec<FacilitySnapshot>,
    pub total_inventory: Vec<(Good, f32)>,
    pub provision_satisfaction: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ShipSnapshot {
    pub id: u64,
    pub name: String,
    pub owner: u64,
    pub status: ShipStatus,
    pub location: u64,
    pub destination: Option<u64>,
    pub days_remaining: u32,
    // Cargo data
    pub cargo: Vec<(Good, f32)>,
    pub capacity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct OrgSnapshot {
    pub id: u64,
    pub name: String,
    pub treasury: f32,
}
