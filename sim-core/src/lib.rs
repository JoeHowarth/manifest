use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

// ============================================================================
// IDs - Using slotmap for generational indices
// ============================================================================

new_key_type! {
    pub struct SettlementId;
    pub struct ShipId;
    pub struct FacilityId;
    pub struct OrgId;
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
// Population - Abstract representation of settlement inhabitants
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Population {
    pub count: u32,
    pub wealth: f32, // Average wealth per capita
}

impl Default for Population {
    fn default() -> Self {
        Self {
            count: 1000,
            wealth: 1.0,
        }
    }
}

// ============================================================================
// Market - Where prices emerge from supply and demand
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct GoodState {
    pub supply: f32,
    pub demand: f32,
    pub price: f32,
}

impl Default for GoodState {
    fn default() -> Self {
        Self {
            supply: 0.0,
            demand: 0.0,
            price: 10.0, // Base price
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Market {
    pub goods: HashMap<Good, GoodState>,
}

// ============================================================================
// Inventory - Goods held by an entity
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub items: HashMap<Good, f32>,
}

impl Inventory {
    pub fn add(&mut self, good: Good, amount: f32) {
        *self.items.entry(good).or_insert(0.0) += amount;
    }

    pub fn remove(&mut self, good: Good, amount: f32) -> f32 {
        let current = self.items.entry(good).or_insert(0.0);
        let removed = amount.min(*current);
        *current -= removed;
        removed
    }

    pub fn get(&self, good: Good) -> f32 {
        self.items.get(&good).copied().unwrap_or(0.0)
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
    pub warehouses: HashMap<u64, Inventory>, // OrgId -> Inventory (using u64 for serialization)
    pub natural_resources: Vec<NaturalResource>,
}

// ============================================================================
// Route - An edge connecting settlements
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum TransportMode {
    Sea,
    River,
    Land,
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct Route {
    pub from: u64, // SettlementId as u64 for serialization
    pub to: u64,
    pub mode: TransportMode,
    pub distance: u32, // In days
    pub risk: f32,     // 0.0 - 1.0, chance of incident per trip
}

// ============================================================================
// Ship - A transport asset
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum ShipStatus {
    InPort,
    EnRoute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ship {
    pub name: String,
    pub owner: u64, // OrgId
    pub capacity: f32,
    pub cargo: Inventory,
    pub status: ShipStatus,
    pub location: u64,          // SettlementId if in port
    pub destination: Option<u64>, // SettlementId if en route
    pub days_remaining: u32,
}

// ============================================================================
// Facility - A production asset
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Facility {
    pub kind: FacilityType,
    pub owner: u64, // OrgId
    pub location: u64, // SettlementId
    pub capacity: f32,
    pub efficiency: f32, // 0.0 - 1.0
    pub workforce: u32,
}

// ============================================================================
// Price Snapshot - Information about market state at a point in time
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PriceSnapshot {
    pub settlement: u64, // SettlementId
    pub tick: u64,
    pub prices: HashMap<Good, f32>,
    pub supply_levels: HashMap<Good, SupplyLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum SupplyLevel {
    Glutted,
    Adequate,
    Scarce,
}

// ============================================================================
// Organization - The player/AI controlled entities
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Org {
    pub name: String,
    pub treasury: f32,
    pub known_prices: HashMap<u64, PriceSnapshot>, // SettlementId -> latest snapshot
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
pub struct SettlementSnapshot {
    pub id: u64,
    pub name: String,
    pub position: (f32, f32),
    pub population: u32,
    pub wealth: f32,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct OrgSnapshot {
    pub id: u64,
    pub name: String,
    pub treasury: f32,
}

// ============================================================================
// WASM API
// ============================================================================

#[wasm_bindgen]
pub struct Simulation {
    state: GameState,
}

#[wasm_bindgen]
impl Simulation {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: GameState::new(),
        }
    }

    /// Create a simulation with a test scenario
    #[wasm_bindgen]
    pub fn with_test_scenario() -> Self {
        let mut sim = Self::new();
        sim.setup_test_scenario();
        sim
    }

    /// Advance the simulation by one tick
    #[wasm_bindgen]
    pub fn advance_tick(&mut self) {
        self.state.tick += 1;
        // TODO: Implement tick logic
        // 1. Production
        // 2. Consumption
        // 3. Transport
        // 4. Market clearing
        // 5. Population update
        // 6. Org update
    }

    /// Get the current tick
    #[wasm_bindgen]
    pub fn get_tick(&self) -> u64 {
        self.state.tick
    }

    /// Get a snapshot of the current state for rendering
    #[wasm_bindgen]
    pub fn get_state_snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            tick: self.state.tick,
            settlements: self
                .state
                .settlements
                .iter()
                .map(|(id, s)| SettlementSnapshot {
                    id: id.0.as_ffi(),
                    name: s.name.clone(),
                    position: s.position,
                    population: s.population.count,
                    wealth: s.population.wealth,
                })
                .collect(),
            routes: self.state.routes.clone(),
            ships: self
                .state
                .ships
                .iter()
                .map(|(id, s)| ShipSnapshot {
                    id: id.0.as_ffi(),
                    name: s.name.clone(),
                    owner: s.owner,
                    status: s.status,
                    location: s.location,
                    destination: s.destination,
                    days_remaining: s.days_remaining,
                })
                .collect(),
            orgs: self
                .state
                .orgs
                .iter()
                .map(|(id, o)| OrgSnapshot {
                    id: id.0.as_ffi(),
                    name: o.name.clone(),
                    treasury: o.treasury,
                })
                .collect(),
        }
    }

    fn setup_test_scenario(&mut self) {
        // Create settlements
        let hartwen = self.state.settlements.insert(Settlement {
            name: "Hartwen".to_string(),
            position: (200.0, 300.0),
            population: Population {
                count: 5000,
                wealth: 1.2,
            },
            market: Market::default(),
            warehouses: HashMap::new(),
            natural_resources: vec![NaturalResource::FertileLand, NaturalResource::Forest],
        });

        let osmouth = self.state.settlements.insert(Settlement {
            name: "Osmouth".to_string(),
            position: (500.0, 300.0),
            population: Population {
                count: 8000,
                wealth: 1.5,
            },
            market: Market::default(),
            warehouses: HashMap::new(),
            natural_resources: vec![NaturalResource::Fishery],
        });

        let millport = self.state.settlements.insert(Settlement {
            name: "Millport".to_string(),
            position: (350.0, 100.0),
            population: Population {
                count: 3000,
                wealth: 1.0,
            },
            market: Market::default(),
            warehouses: HashMap::new(),
            natural_resources: vec![NaturalResource::Forest, NaturalResource::OreDeposit],
        });

        let greyvale = self.state.settlements.insert(Settlement {
            name: "Greyvale".to_string(),
            position: (150.0, 500.0),
            population: Population {
                count: 2000,
                wealth: 0.8,
            },
            market: Market::default(),
            warehouses: HashMap::new(),
            natural_resources: vec![NaturalResource::Pastureland],
        });

        let dunmere = self.state.settlements.insert(Settlement {
            name: "Dunmere".to_string(),
            position: (550.0, 500.0),
            population: Population {
                count: 4000,
                wealth: 1.1,
            },
            market: Market::default(),
            warehouses: HashMap::new(),
            natural_resources: vec![NaturalResource::OreDeposit],
        });

        // Create routes
        self.state.routes.push(Route {
            from: hartwen.0.as_ffi(),
            to: osmouth.0.as_ffi(),
            mode: TransportMode::Sea,
            distance: 5,
            risk: 0.05,
        });

        self.state.routes.push(Route {
            from: hartwen.0.as_ffi(),
            to: millport.0.as_ffi(),
            mode: TransportMode::River,
            distance: 3,
            risk: 0.02,
        });

        self.state.routes.push(Route {
            from: hartwen.0.as_ffi(),
            to: greyvale.0.as_ffi(),
            mode: TransportMode::Land,
            distance: 2,
            risk: 0.01,
        });

        self.state.routes.push(Route {
            from: osmouth.0.as_ffi(),
            to: dunmere.0.as_ffi(),
            mode: TransportMode::Sea,
            distance: 4,
            risk: 0.04,
        });

        self.state.routes.push(Route {
            from: millport.0.as_ffi(),
            to: osmouth.0.as_ffi(),
            mode: TransportMode::River,
            distance: 4,
            risk: 0.03,
        });

        // Create a player org
        let player_org = self.state.orgs.insert(Org {
            name: "Player Trading Co.".to_string(),
            treasury: 10000.0,
            known_prices: HashMap::new(),
        });

        // Create a ship for the player
        self.state.ships.insert(Ship {
            name: "Maiden's Fortune".to_string(),
            owner: player_org.0.as_ffi(),
            capacity: 100.0,
            cargo: Inventory::default(),
            status: ShipStatus::InPort,
            location: hartwen.0.as_ffi(),
            destination: None,
            days_remaining: 0,
        });

        self.state.ships.insert(Ship {
            name: "Sea Drake".to_string(),
            owner: player_org.0.as_ffi(),
            capacity: 150.0,
            cargo: Inventory::default(),
            status: ShipStatus::InPort,
            location: osmouth.0.as_ffi(),
            destination: None,
            days_remaining: 0,
        });
    }
}

impl Default for Simulation {
    fn default() -> Self {
        Self::new()
    }
}
