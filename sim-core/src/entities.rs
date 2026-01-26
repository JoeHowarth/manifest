use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tsify_next::Tsify;

use crate::market::{Inventory, LaborMarket, Market};
use crate::types::{FacilityType, NaturalResource, ShipStatus, TransportMode};

// ============================================================================
// Population - Abstract representation of settlement inhabitants
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Population {
    pub count: u32,
    pub wealth: f32, // Average wealth per capita
    pub income: f32, // Wages earned this tick (reset each tick)
}

impl Default for Population {
    fn default() -> Self {
        Self {
            count: 1000,
            wealth: 1.0,
            income: 0.0,
        }
    }
}

// ============================================================================
// Settlement - A node in the trade network
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settlement {
    pub name: String,
    pub position: (f32, f32), // For rendering
    pub population: Population,
    pub market: Market,
    pub labor_market: LaborMarket,
    pub warehouses: HashMap<u64, Inventory>, // OrgId -> Inventory
    pub natural_resources: Vec<NaturalResource>,
    pub facility_ids: Vec<u64>, // FacilityIds at this settlement (reverse index)
}

// ============================================================================
// Route - An edge connecting settlements
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct Route {
    pub from: u64, // SettlementId
    pub to: u64,
    pub mode: TransportMode,
    pub distance: u32, // In days
    pub risk: f32,     // 0.0 - 1.0, chance of incident per trip
}

// ============================================================================
// Ship - A transport asset
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ship {
    pub name: String,
    pub owner: u64, // OrgId
    pub capacity: f32,
    pub cargo: Inventory,
    pub status: ShipStatus,
    pub location: u64,            // SettlementId if in port
    pub destination: Option<u64>, // SettlementId if en route
    pub days_remaining: u32,
}

// ============================================================================
// Facility - A production asset
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Facility {
    pub kind: FacilityType,
    pub owner: u64,             // OrgId
    pub location: u64,          // SettlementId
    pub optimal_workforce: u32, // Workers needed for peak efficiency
    pub current_workforce: u32, // Workers actually allocated this tick
    pub efficiency: f32,        // 0.0 - 1.0, affected by tools etc.
}

// ============================================================================
// Organization - The player/AI controlled entities
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Org {
    pub name: String,
    pub treasury: f32,
    // Note: For v1, full information - all orgs can see all prices
}
