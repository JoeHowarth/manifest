use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tsify_next::Tsify;

use crate::market::LaborMarket;
use crate::types::{
    FacilityType, Good, LocationId, NaturalResource, OrgType, ShipStatus, TransportMode,
};

// ============================================================================
// Population - Abstract representation of settlement inhabitants
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Population {
    pub count: u32,
    pub wealth: f32, // Total money savings (the stock)

    // Household stockpiles (separate from org stockpiles)
    pub stockpile_provisions: f32, // Household food reserves

    // Targets for behavior curves
    pub target_wealth: f32,     // Comfortable savings level
    pub target_provisions: f32, // Desired food buffer
}

impl Default for Population {
    fn default() -> Self {
        Self {
            count: 1000,
            wealth: 50000.0,              // Start with ~2.5 ticks of wages (wage=20)
            stockpile_provisions: 2000.0, // 2 ticks buffer (1 per person per tick)
            target_wealth: 50000.0,       // ~2.5 ticks of wages
            target_provisions: 2000.0,    // 2 ticks buffer
        }
    }
}

impl Population {
    /// Create a population with count and scaled defaults
    /// Consumption: 1 provision per person per tick
    pub fn with_count(count: u32) -> Self {
        let pop = count as f32;
        Self {
            count,
            wealth: pop * 50.0,              // ~2.5 ticks of wages
            stockpile_provisions: pop * 2.0, // 2 ticks buffer
            target_wealth: pop * 50.0,
            target_provisions: pop * 2.0,
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
    pub labor_market: LaborMarket,
    pub natural_resources: Vec<NaturalResource>,
    pub facility_ids: Vec<u64>, // FacilityIds at this settlement (reverse index)
    pub org_id: Option<u64>,    // Settlement Org that owns subsistence farm
    pub subsistence_capacity: u32, // Max population sustainable by subsistence alone
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
    // Note: Ship cargo is stored as a Stockpile keyed by (owner, LocationId::Ship(ship_id))
    // Access via state.get_stockpile(org_id, LocationId::from_ship(ship_id))
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
    pub org_type: OrgType, // v2: Regular or Settlement
                           // Note: For v1, full information - all orgs can see all prices
}

impl Org {
    pub fn new_regular(name: String, treasury: f32) -> Self {
        Self {
            name,
            treasury,
            org_type: OrgType::Regular,
        }
    }

    pub fn new_settlement(name: String, treasury: f32) -> Self {
        Self {
            name,
            treasury,
            org_type: OrgType::Settlement,
        }
    }
}

// ============================================================================
// Stockpile - Inventory at a specific location (v2)
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stockpile {
    pub goods: HashMap<Good, f32>,
}

impl Stockpile {
    pub fn new() -> Self {
        Self {
            goods: HashMap::new(),
        }
    }

    pub fn add(&mut self, good: Good, amount: f32) {
        *self.goods.entry(good).or_insert(0.0) += amount;
    }

    pub fn remove(&mut self, good: Good, amount: f32) -> f32 {
        let current = self.goods.entry(good).or_insert(0.0);
        let removed = amount.min(*current);
        *current -= removed;
        removed
    }

    pub fn get(&self, good: Good) -> f32 {
        self.goods.get(&good).copied().unwrap_or(0.0)
    }

    pub fn total(&self) -> f32 {
        self.goods.values().sum()
    }
}

/// Key for stockpile lookup: (org_id as u64, location)
pub type StockpileKey = (u64, LocationId);
