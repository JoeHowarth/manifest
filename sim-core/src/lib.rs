//! Economic simulation library
//!
//! This crate implements a bottom-up economic simulation with two agent types:
//!
//! ## Pops (Population Units)
//!
//! A **Pop** represents ~100 workers plus their non-working dependents. Pops are the
//! "atoms" of population — mechanical entities that follow economic rules without
//! player/AI control. Each pop:
//!
//! - Is permanently bound to a **home settlement** (works, consumes, and trades there)
//! - Makes **consumption decisions** using utility curves that model diminishing returns
//! - Participates in the **labor market** as a single worker unit
//! - Maintains an inline **stockpile** of goods at their home settlement
//! - Tracks **income EMA** to smooth wage fluctuations into spending decisions
//!
//! Settlement size = number of pops. A town of 5,000 people ≈ 50 pops.
//!
//! ## Merchants
//!
//! A **Merchant** is the primary agent with real agency — controlled by players or AI bots.
//! Unlike pops, merchants operate across the entire world:
//!
//! - **Facilities**: Own production facilities at settlements. Facilities enable stockpiling
//!   and production at that location.
//! - **Trade routes**: Move goods between settlements. Routes without a facility at the
//!   destination force immediate sale upon arrival (no storage without facility).
//! - **Market participation**: Can buy/sell at any settlement where they have presence
//!   (facility OR active trade route).
//!
//! Merchants will eventually have a clean interface for bot/player control, separate from
//! the core simulation logic.
//!
//! ## Markets
//!
//! Markets are **per-settlement**. Each settlement has its own commodity market where:
//! - Local pops generate supply/demand curves based on their needs and stocks
//! - Merchants with presence submit orders
//! - A **call auction** clears all orders simultaneously, finding the price that
//!   maximizes traded volume
//! - Budget constraints are enforced with iterative relaxation
//!
//! Cross-settlement trade happens via merchant trade routes, creating arbitrage
//! opportunities that drive price convergence.
//!
//! ## Forward Direction
//!
//! - **Production**: Facilities consume inputs and produce outputs using recipes.
//!   Natural resources at settlements gate what can be produced where.
//! - **Trade routes**: Merchants configure routes to buy goods at one settlement
//!   and sell at another, with carts allocated to each route.
//! - **Labor market integration**: Pops work at facilities, wages flow to pops,
//!   production output flows to facility owners.
//! - **Merchant AI interface**: Clean separation between simulation and decision-making
//!   to allow pluggable AI or player control.
//!
//! ## Module Structure
//!
//! - `types`       Core type definitions (IDs, goods)
//! - `geography`   Settlement and route definitions
//! - `agents`      Pop and merchant agent types
//! - `production`  Recipe and facility definitions
//! - `labor`       Skill-based labor market
//! - `consumption` Utility-based consumption model
//! - `market`      Auction-based market clearing
//! - `needs`       Need and utility curve definitions
//! - `tick`        Full simulation tick orchestration
//! - `world`       World state container

pub mod accounting;
pub mod agents;
pub mod consumption;
pub mod external;
pub mod geography;
#[cfg(feature = "instrument")]
pub use instrument;
pub mod labor;
pub mod market;
pub mod mortality;
pub mod needs;
pub mod production;
pub mod tick;
pub mod types;
pub mod world;

// Re-export commonly used types at the crate root

// Accounting
pub use accounting::{TickStockFlow, WorldFlowSnapshot, capture_world_flow_snapshot, decompose_tick_flow};

// Core types
pub use types::{
    AgentId, FacilityId, GoodId, GoodProfile, MerchantId, NeedContribution, PopId, Price, Quantity,
    SettlementId,
};

// Agents
pub use agents::{ConsumptionResult, MerchantAgent, Pop, Stockpile};

// Geography
pub use geography::{ResourceQuality, ResourceSlot, ResourceType, Route, Settlement};

// External market
pub use external::{
    AnchoredGoodConfig, ExternalMarketConfig, OutsideFlowTotals, SettlementFriction,
};

// Production
pub use production::{
    Facility, FacilityDef, FacilityType, Recipe, RecipeId, get_facility_def, get_facility_defs,
};

// World
pub use world::World;

// Consumption
pub use consumption::{compute_consumption, greedy_consume};

// Labor
pub use labor::{
    Assignment, ComplementaryProductionFn, LaborAsk, LaborBid, LaborMarketResult, ProductionFn,
    SkillDef, SkillId, SubsistenceReservationConfig, Worker, WorkerId, apply_assignments,
    build_subsistence_reservation_ladder, clear_labor_markets, generate_facility_bids,
    generate_pop_asks_with_min_wage, generate_worker_asks, update_wage_emas,
};

// Market
pub use market::{
    Fill, MarketClearResult, MultiMarketResult, Order, PriceBias, Side, apply_fill,
    apply_fill_merchant, clear_multi_market, clear_single_market,
};

// Needs
pub use needs::{Need, UtilityCurve};

// Tick
#[allow(deprecated)]
pub use tick::run_market_tick;
pub use tick::run_settlement_tick;
pub use tick::{BUFFER_TICKS, PRICE_SWEEP_MAX, PRICE_SWEEP_MIN, PRICE_SWEEP_POINTS};
pub use tick::{generate_demand_curve_orders, qty_norm, qty_sell};

// Mortality
pub use mortality::{MortalityOutcome, check_mortality, death_probability, growth_probability};
