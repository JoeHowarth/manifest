// Population economic simulation module
//
// Module structure:
// - types/       Core type definitions (IDs, goods, locations)
// - geography/   Settlement and route definitions
// - agents/      Population and merchant agent types
// - production/  Recipe and facility definitions
// - labor/       Skill-based labor market
// - consumption/ Utility-based consumption model
// - market/      Auction-based market clearing
// - needs        Need and utility curve definitions
// - tick         Full simulation tick orchestration
// - world        World state container

pub mod agents;
pub mod consumption;
pub mod geography;
pub mod labor;
pub mod market;
pub mod needs;
pub mod production;
pub mod tick;
pub mod types;
pub mod world;

// Re-export commonly used types at the module root
//
// IMPORTANT: The following types are NOT re-exported to avoid conflicts with parent's types:
// - SettlementId, LocationId (use pops::types::SettlementId, pops::types::LocationId)
// - Settlement, Route, NaturalResource (use pops::geography::*)
// - Stockpile, StockpileKey (use pops::agents::Stockpile, pops::agents::StockpileKey)
// - FacilityId, Facility (use pops::labor::FacilityId, pops::labor::Facility)

// Core types (only non-conflicting ones)
pub use types::{AgentId, GoodId, GoodProfile, NeedContribution, Price, Quantity};

// Agents (only non-conflicting ones)
pub use agents::{ConsumptionResult, MerchantAgent, PopulationState};

// World
pub use world::World;

// Consumption
pub use consumption::{compute_consumption, greedy_consume};

// Labor
pub use labor::{
    apply_assignments, clear_labor_markets, generate_facility_bids, generate_worker_asks,
    update_wage_emas, Assignment, ComplementaryProductionFn, LaborBid, LaborAsk,
    LaborMarketResult, ProductionFn, SkillDef, SkillId, Worker, WorkerId,
};

// Market
pub use market::{
    apply_fill, apply_fill_merchant, clear_multi_market, clear_single_market, Fill,
    MarketClearResult, MultiMarketResult, Order, PriceBias, Side,
};

// Needs
pub use needs::{Need, UtilityCurve};

// Tick
pub use tick::run_market_tick;
