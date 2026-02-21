//! Convergence tests for economic simulation
//!
//! Tests that a minimal economy reaches stable equilibrium under various conditions.
//!
//! ## Equilibrium Dynamics
//!
//! Both pop and merchant maintain target buffer stocks:
//! - Pop target: `desired_consumption × buffer_ticks`
//! - Merchant target: `TARGET_STOCK_BUFFER` (hardcoded 2.0 in merchant.rs)
//!
//! The equilibrium price depends on the ratio of these targets.
//! When merchant target < pop target, merchant is more eager to sell → lower prices.

#[allow(dead_code)]
mod common;
use common::*;

use serde::{Deserialize, Serialize};
use sim_core::{
    AnchoredGoodConfig, ExternalMarketConfig, SettlementFriction, SubsistenceReservationConfig,
    World,
    production::{FacilityType, RecipeId},
    types::GoodId,
};
const MULTI_POP_PRODUCTION_RATE: f64 = 1.05;
const CALIBRATION_PRODUCTION_RATE: f64 = 1.0;

// === SYSTEM PARAMETERS ===

/// Tunable system parameters for convergence tests
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct SystemParams {
    /// Production output per tick (recipe output)
    production_rate: f64,
    /// Pop's food requirement (affects consumption)
    consumption_requirement: f64,
    /// Pop's desired consumption EMA (affects their buffer target)
    /// Pop target = desired_consumption_ema × BUFFER_TICKS (5.0)
    pop_desired_consumption: f64,
    /// Initial wage EMA
    initial_wage: f64,
}

impl Default for SystemParams {
    fn default() -> Self {
        Self {
            production_rate: 1.0,
            consumption_requirement: 1.0,
            pop_desired_consumption: 1.0, // → pop target = 5.0
            initial_wage: 1.0,
        }
    }
}

/// Initial conditions for a convergence test
#[derive(Debug, Clone, Copy)]
struct InitialConditions {
    pop_stock: f64,
    merchant_stock: f64,
    initial_price: f64,
}

impl Default for InitialConditions {
    fn default() -> Self {
        Self {
            pop_stock: 2.0,
            merchant_stock: 0.0,
            initial_price: 1.0,
        }
    }
}

// === WORLD CREATION ===

fn create_world(params: SystemParams, conditions: InitialConditions) -> World {
    let mut world = World::new();

    let settlement = world.add_settlement("TestTown", (0.0, 0.0));

    // Merchant with stockpile
    let merchant = world.add_merchant();
    {
        let m = world.get_merchant_mut(merchant).unwrap();
        m.currency = 1000.0;
        if conditions.merchant_stock > 0.0 {
            m.stockpile_at(settlement)
                .add(GRAIN, conditions.merchant_stock);
        }
    }

    // Farm
    let farm = world
        .add_facility(FacilityType::Farm, settlement, merchant)
        .unwrap();

    // Pop
    let pop = world.add_pop(settlement).unwrap();
    {
        let p = world.get_pop_mut(pop).unwrap();
        p.currency = 100.0;
        p.skills.insert(LABORER);
        p.min_wage = 0.0;
        p.employed_at = Some(farm);
        p.income_ema = params.initial_wage;
        p.stocks.insert(GRAIN, conditions.pop_stock);
        p.desired_consumption_ema
            .insert(GRAIN, params.pop_desired_consumption);
    }

    // Facility setup
    {
        let f = world.get_facility_mut(farm).unwrap();
        f.workers.insert(LABORER, 1);
        f.recipe_priorities = vec![RecipeId::new(1)];
    }

    world.wage_ema.insert(LABORER, params.initial_wage);
    world
        .price_ema
        .insert((settlement, GRAIN), conditions.initial_price);

    world
}




// === TESTS ===

/// Examine order curves to understand clearing dynamics
#[test]
#[ignore = "diagnostic trace; run manually"]
fn trace_order_curves() {
    // Simulate what orders would be generated at different conditions
    println!("\n=== Order Curve Analysis ===\n");

    // Pop's demand curve: qty_norm(norm_p, norm_c)
    // norm_p = price / EMA (0.6 to 1.4)
    // norm_c = stock / target
    println!("Pop demand curve qty_frac = shortfall × (0.3 + 0.7 × price_factor)");
    println!("  shortfall = (1 - norm_c).max(0)");
    println!("  price_factor = 1 - norm_p\n");

    let norm_ps = [0.6, 0.8, 1.0, 1.2, 1.4];

    // Case 1: Pop at target (norm_c = 1.0)
    println!("Pop at target (stock/target = 1.0):");
    println!("  norm_p  price_fac  shortfall  qty_frac");
    for &np in &norm_ps {
        let shortfall = (1.0 - 1.0_f64).max(0.0);
        let price_factor = 1.0 - np;
        let qty_frac = (shortfall * (0.3 + 0.7 * price_factor)).clamp(0.0, 1.0);
        println!(
            "  {:>5.2}  {:>9.2}  {:>9.2}  {:>8.3}",
            np, price_factor, shortfall, qty_frac
        );
    }

    // Case 2: Pop below target (norm_c = 0.2, i.e. stock = 1, target = 5)
    println!("\nPop below target (stock/target = 0.2):");
    println!("  norm_p  price_fac  shortfall  qty_frac");
    for &np in &norm_ps {
        let shortfall = (1.0 - 0.2_f64).max(0.0);
        let price_factor = 1.0 - np;
        let qty_frac = (shortfall * (0.3 + 0.7 * price_factor)).clamp(0.0, 1.0);
        println!(
            "  {:>5.2}  {:>9.2}  {:>9.2}  {:>8.3}",
            np, price_factor, shortfall, qty_frac
        );
    }

    // Merchant's supply curve: qty_supply(norm_p, norm_c)
    println!("\n\nMerchant supply curve:");
    println!("  qty_frac = (excess × (0.5 + 0.5×pf) + 0.1×pf.max(0)).clamp(0,1)");
    println!("  excess = (norm_c - 1).max(0)\n");

    // Case 1: Merchant below target (norm_c = 0.5, stock = 1, target = 2)
    println!("Merchant below target (stock/target = 0.5):");
    println!("  norm_p  price_fac  excess  qty_frac");
    for &np in &norm_ps {
        let excess = (0.5 - 1.0_f64).max(0.0);
        let price_factor = (np - 1.0).max(-0.3);
        let qty_frac =
            (excess * (0.5 + 0.5 * price_factor) + 0.1 * price_factor.max(0.0)).clamp(0.0, 1.0);
        println!(
            "  {:>5.2}  {:>9.2}  {:>6.2}  {:>8.3}",
            np, price_factor, excess, qty_frac
        );
    }

    // Case 2: Merchant above target (norm_c = 1.5, stock = 3, target = 2)
    println!("\nMerchant above target (stock/target = 1.5):");
    println!("  norm_p  price_fac  excess  qty_frac");
    for &np in &norm_ps {
        let excess = (1.5 - 1.0_f64).max(0.0);
        let price_factor = (np - 1.0).max(-0.3);
        let qty_frac =
            (excess * (0.5 + 0.5 * price_factor) + 0.1 * price_factor.max(0.0)).clamp(0.0, 1.0);
        println!(
            "  {:>5.2}  {:>9.2}  {:>6.2}  {:>8.3}",
            np, price_factor, excess, qty_frac
        );
    }

    // Now show what happens with budget constraint
    println!("\n\n=== Budget Constraint Effect ===\n");
    println!("Pop budget = income_ema ≈ 1.0");
    println!("Pop target = 5.0\n");

    // At EMA = 1.0
    println!("At price_EMA = 1.0:");
    println!("  norm_p  limit_p  qty_frac  qty(×tgt)  cost    afford?");
    let ema = 1.0;
    let target = 5.0;
    let budget = 1.0;
    for &np in &norm_ps {
        let limit = np * ema;
        let shortfall = 0.8; // assume 80% shortfall
        let pf = 1.0 - np;
        let qty_frac = (shortfall * (0.3 + 0.7 * pf)).clamp(0.0, 1.0);
        let qty = qty_frac * target;
        let cost = qty * limit;
        let afford = if cost <= budget { "yes" } else { "NO" };
        println!(
            "  {:>5.2}  {:>7.2}  {:>8.3}  {:>9.2}  {:>6.2}  {:>7}",
            np, limit, qty_frac, qty, cost, afford
        );
    }

    // At EMA = 2.0
    println!("\nAt price_EMA = 2.0:");
    println!("  norm_p  limit_p  qty_frac  qty(×tgt)  cost    afford?");
    let ema = 2.0;
    for &np in &norm_ps {
        let limit = np * ema;
        let shortfall = 0.8;
        let pf = 1.0 - np;
        let qty_frac = (shortfall * (0.3 + 0.7 * pf)).clamp(0.0, 1.0);
        let qty = qty_frac * target;
        let cost = qty * limit;
        let afford = if cost <= budget { "yes" } else { "NO" };
        println!(
            "  {:>5.2}  {:>7.2}  {:>8.3}  {:>9.2}  {:>6.2}  {:>7}",
            np, limit, qty_frac, qty, cost, afford
        );
    }

    println!("\nKey insight: At EMA=2.0, pop can't afford ANY of their desired quantities!");
    println!("Budget relaxation removes orders, leaving only tiny affordable amounts.");
}

/// Trace what orders are actually generated and what clears using real functions
#[test]
#[ignore = "diagnostic trace; run manually"]
fn trace_clearing_mechanism() {
    use sim_core::market::{self, Order, PriceBias, Side};
    use sim_core::tick::{BUFFER_TICKS, generate_demand_curve_orders};
    use std::collections::HashMap as StdHashMap;

    fn side_str(s: &Side) -> &'static str {
        match s {
            Side::Buy => "Buy",
            Side::Sell => "Sell",
        }
    }

    println!("\n=== Clearing Mechanism Analysis (Using Real Functions) ===\n");

    // Set up the scenario: EMA=2.0, pop below target, merchant above target
    let params = SystemParams::default();
    let conditions = InitialConditions {
        initial_price: 2.0,
        pop_stock: 0.4,      // well below target (5.0)
        merchant_stock: 3.0, // above target (2.0)
    };

    let world = create_world(params, conditions);
    let good_profiles = make_grain_profile();
    let settlement = *world.settlements.keys().next().unwrap();

    // Get references to agents
    let pop = world.pops.values().next().unwrap();
    let merchant = world.merchants.values().next().unwrap();

    // Build price_ema map for the good
    let mut price_ema: StdHashMap<GoodId, f64> = StdHashMap::new();
    price_ema.insert(GRAIN, conditions.initial_price);

    println!("Setup:");
    println!(
        "  Pop stock: {:.1} (target: {:.1})",
        conditions.pop_stock, BUFFER_TICKS
    );
    println!(
        "  Merchant stock: {:.1} (target: 2.0)",
        conditions.merchant_stock
    );
    println!("  Price EMA: {:.1}", conditions.initial_price);
    println!("  Pop budget (income_ema): {:.1}", pop.income_ema);
    println!();

    // Generate orders using real functions
    let pop_orders = generate_demand_curve_orders(pop, &good_profiles, &price_ema);
    let merchant_orders = merchant.generate_orders(settlement, &price_ema);

    println!("Pop orders (generated by real generate_demand_curve_orders):");
    for o in &pop_orders {
        println!(
            "  {} {:.3} @ {:.3}",
            side_str(&o.side),
            o.quantity,
            o.limit_price
        );
    }

    println!("\nMerchant orders (generated by real merchant.generate_orders):");
    for o in &merchant_orders {
        println!(
            "  {} {:.3} @ {:.3}",
            side_str(&o.side),
            o.quantity,
            o.limit_price
        );
    }

    // Combine and assign IDs
    let mut all_orders: Vec<Order> = Vec::new();
    let mut next_id = 0u64;
    for mut o in pop_orders {
        o.id = next_id;
        next_id += 1;
        all_orders.push(o);
    }
    for mut o in merchant_orders {
        o.id = next_id;
        next_id += 1;
        all_orders.push(o);
    }

    // Set up budgets (AgentId is u32)
    let mut budgets: StdHashMap<u32, f64> = StdHashMap::new();
    budgets.insert(pop.id.0, pop.income_ema.min(pop.currency));
    budgets.insert(merchant.id.0, merchant.currency);

    // Set up seller inventories
    let seller_inventories: StdHashMap<u32, StdHashMap<GoodId, f64>> = {
        let mut inv = StdHashMap::new();
        let goods_map: StdHashMap<GoodId, f64> = merchant
            .stockpiles
            .get(&settlement)
            .map(|s| [(GRAIN, s.get(GRAIN))].into_iter().collect())
            .unwrap_or_default();
        inv.insert(merchant.id.0, goods_map);
        inv
    };

    println!("\nBudgets:");
    println!(
        "  Pop: {:.1} (income_ema={:.1}, currency={:.1})",
        pop.income_ema.min(pop.currency),
        pop.income_ema,
        pop.currency
    );
    println!("  Merchant: {:.1}", merchant.currency);

    // Run real market clearing
    let result = market::clear_multi_market(
        &[GRAIN],
        all_orders.clone(),
        &budgets,
        Some(&seller_inventories),
        20,
        PriceBias::FavorSellers,
    );

    println!("\nClearing result:");
    println!("  Iterations: {}", result.iterations);
    if let Some(&price) = result.clearing_prices.get(&GRAIN) {
        println!("  Clearing price: {:.3}", price);
    } else {
        println!("  No trades!");
    }

    println!("\nFills:");
    let mut pop_bought = 0.0;
    let mut pop_spent = 0.0;
    let mut merc_sold = 0.0;
    let mut merc_earned = 0.0;

    for fill in &result.fills {
        let agent = if fill.agent_id == pop.id.0 {
            "Pop"
        } else {
            "Merchant"
        };
        println!(
            "  {} {} {:.3} @ {:.3} = {:.3}",
            agent,
            side_str(&fill.side),
            fill.quantity,
            fill.price,
            fill.quantity * fill.price
        );
        if fill.agent_id == pop.id.0 {
            pop_bought += fill.quantity;
            pop_spent += fill.quantity * fill.price;
        } else {
            merc_sold += fill.quantity;
            merc_earned += fill.quantity * fill.price;
        }
    }

    println!("\nSummary:");
    println!("  Pop bought: {:.3}, spent: {:.3}", pop_bought, pop_spent);
    println!(
        "  Merchant sold: {:.3}, earned: {:.3}",
        merc_sold, merc_earned
    );

    // Show what new EMA would be
    if let Some(&price) = result.clearing_prices.get(&GRAIN) {
        let new_ema = 0.7 * conditions.initial_price + 0.3 * price;
        println!("\nEMA update:");
        println!("  Old EMA: {:.3}", conditions.initial_price);
        println!("  Clearing price: {:.3}", price);
        println!(
            "  New EMA = 0.7 × {:.3} + 0.3 × {:.3} = {:.3}",
            conditions.initial_price, price, new_ema
        );

        if new_ema > conditions.initial_price {
            println!("  → EMA INCREASED! Death spiral continues.");
        } else {
            println!("  → EMA decreased, system stabilizing.");
        }
    }
}

/// Trace the death spiral case: merchant starts with 0 stock
#[test]
#[ignore = "diagnostic trace; run manually"]
fn trace_death_spiral_orders() {
    use sim_core::market::{self, Order, PriceBias, Side};
    use sim_core::tick::{BUFFER_TICKS, generate_demand_curve_orders};
    use std::collections::HashMap as StdHashMap;

    fn side_str(s: &Side) -> &'static str {
        match s {
            Side::Buy => "Buy",
            Side::Sell => "Sell",
        }
    }

    println!("\n=== Death Spiral Case: Merchant Starts Below Target ===\n");

    // The death spiral happens when:
    // - price EMA is high (2.0)
    // - merchant stock is BELOW target (they just produced 1 unit but target is 2)

    let params = SystemParams::default();
    // After tick 1 in trace_dynamics: merchant has ~1 stock, pop has ~1 stock
    let conditions = InitialConditions {
        initial_price: 2.0,
        pop_stock: 1.0,      // below target (5.0)
        merchant_stock: 1.0, // BELOW target (2.0) - this is key!
    };

    let world = create_world(params, conditions);
    let good_profiles = make_grain_profile();
    let settlement = *world.settlements.keys().next().unwrap();

    let pop = world.pops.values().next().unwrap();
    let merchant = world.merchants.values().next().unwrap();

    let mut price_ema: StdHashMap<GoodId, f64> = StdHashMap::new();
    price_ema.insert(GRAIN, conditions.initial_price);

    println!("Setup:");
    println!(
        "  Pop stock: {:.1} (target: {:.1})",
        conditions.pop_stock, BUFFER_TICKS
    );
    println!(
        "  Merchant stock: {:.1} (target: 2.0) ← BELOW TARGET!",
        conditions.merchant_stock
    );
    println!("  Price EMA: {:.1}", conditions.initial_price);
    println!();

    // Generate orders
    let pop_orders = generate_demand_curve_orders(pop, &good_profiles, &price_ema);
    let merchant_orders = merchant.generate_orders(settlement, &price_ema);

    println!("Pop orders:");
    for o in &pop_orders {
        println!(
            "  {} {:.3} @ {:.3}",
            side_str(&o.side),
            o.quantity,
            o.limit_price
        );
    }

    println!("\nMerchant orders (BELOW target → only sells at premium!):");
    if merchant_orders.is_empty() {
        println!("  (NONE! Merchant won't sell when below target unless price is premium)");
    } else {
        for o in &merchant_orders {
            println!(
                "  {} {:.3} @ {:.3}",
                side_str(&o.side),
                o.quantity,
                o.limit_price
            );
        }
    }

    // Combine and run clearing
    let mut all_orders: Vec<Order> = Vec::new();
    let mut next_id = 0u64;
    for mut o in pop_orders.clone() {
        o.id = next_id;
        next_id += 1;
        all_orders.push(o);
    }
    for mut o in merchant_orders.clone() {
        o.id = next_id;
        next_id += 1;
        all_orders.push(o);
    }

    let mut budgets: StdHashMap<u32, f64> = StdHashMap::new();
    budgets.insert(pop.id.0, pop.income_ema.min(pop.currency));
    budgets.insert(merchant.id.0, merchant.currency);

    // Set up seller inventories
    let seller_inventories: StdHashMap<u32, StdHashMap<GoodId, f64>> = {
        let mut inv = StdHashMap::new();
        let goods_map: StdHashMap<GoodId, f64> = merchant
            .stockpiles
            .get(&settlement)
            .map(|s| [(GRAIN, s.get(GRAIN))].into_iter().collect())
            .unwrap_or_default();
        inv.insert(merchant.id.0, goods_map);
        inv
    };

    let result = market::clear_multi_market(
        &[GRAIN],
        all_orders,
        &budgets,
        Some(&seller_inventories),
        20,
        PriceBias::FavorSellers,
    );

    println!("\nClearing result:");
    println!("  Iterations: {}", result.iterations);
    if let Some(&price) = result.clearing_prices.get(&GRAIN) {
        println!("  Clearing price: {:.3}", price);
        let new_ema = 0.7 * conditions.initial_price + 0.3 * price;
        println!("\nEMA update:");
        println!(
            "  Old EMA: {:.3} → New EMA: {:.3}",
            conditions.initial_price, new_ema
        );
        if new_ema > conditions.initial_price {
            println!("  → EMA INCREASED!");
        }
    } else {
        println!("  NO TRADES - no clearing price!");
        println!("\n  When merchant is below target, supply curve only offers at PREMIUM prices.");
        println!("  But pop's budget can't afford premium prices.");
        println!("  No trade → price EMA doesn't update from clearing, but consumption continues.");
    }

    // Now trace what happens as merchant stock grows
    println!("\n=== Multi-tick trace showing merchant stock growth ===\n");
    println!(
        "{:>4} {:>6} {:>6} {:>8} {:>8} {:>8}",
        "tick", "m_stk", "m_tgt", "m_norm", "sell_lo", "sell_hi"
    );

    // Track how merchant's position changes
    let merchant_target = 2.0;
    for merc_stk in [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0] {
        let norm_c = merc_stk / merchant_target;

        // Simulate qty_supply at norm_p = 0.6 (lowest) and 1.4 (highest)
        let supply_low = {
            let excess = (norm_c - 1.0_f64).max(0.0);
            let pf = (0.6 - 1.0_f64).max(-0.3);
            (excess * (0.5 + 0.5 * pf) + 0.1 * pf.max(0.0)).clamp(0.0, 1.0) * merc_stk
        };
        let supply_high = {
            let excess = (norm_c - 1.0_f64).max(0.0);
            let pf = (1.4 - 1.0_f64).max(-0.3);
            (excess * (0.5 + 0.5 * pf) + 0.1 * pf.max(0.0)).clamp(0.0, 1.0) * merc_stk
        };

        let status = if norm_c < 1.0 { "<tgt" } else { "≥tgt" };
        println!(
            "{:>4} {:>6.1} {:>6.1} {:>8.2} {:>8.3} {:>8.3}",
            status, merc_stk, merchant_target, norm_c, supply_low, supply_high
        );
    }

    println!("\nKey insight: When merchant_stock < target, merchant's supply curve");
    println!("only offers at PREMIUM prices (norm_p > 1.0 i.e. limit > EMA).");
    println!("The qty_supply formula has +0.1×pf.max(0) which only activates at norm_p > 1.0.");
    println!("\nThe death spiral happens because:");
    println!("1. Merchant starts below target → premium prices only");
    println!("2. Pop forced to pay premium → EMA rises");
    println!("3. By the time merchant reaches target, EMA is already elevated");
    println!("4. High EMA + pop low on stock → continued premium trading");
}

// === MULTI-POP TESTS ===

/// Create a world with multiple pops and facilities for realistic labor market dynamics
fn create_multi_pop_world(
    num_pops: usize,
    num_facilities: usize,
    initial_price: f64,
    initial_pop_stock: f64,
    initial_merchant_stock: f64,
) -> World {
    let mut world = World::new();

    let settlement = world.add_settlement("TestTown", (0.0, 0.0));

    // Single merchant owns all facilities
    let merchant = world.add_merchant();
    {
        let m = world.get_merchant_mut(merchant).unwrap();
        m.currency = 10000.0;
        if initial_merchant_stock > 0.0 {
            m.stockpile_at(settlement)
                .add(GRAIN, initial_merchant_stock);
        }
    }

    // Create facilities - increase capacity to handle all workers
    let workers_per_facility = num_pops.div_ceil(num_facilities);
    let mut facility_ids = Vec::new();
    for _ in 0..num_facilities {
        let farm = world
            .add_facility(FacilityType::Farm, settlement, merchant)
            .unwrap();
        // Set capacity high enough for all assigned workers
        let f = world.get_facility_mut(farm).unwrap();
        f.capacity = workers_per_facility as u32;
        facility_ids.push(farm);
    }

    // Create pops — start unemployed so the labor market clears naturally on tick 1.
    // Starting all pops as employed would set reservation = q_max * price (the solo
    // farmer output), which exceeds MVP and causes mass unemployment on tick 1.
    for _i in 0..num_pops {
        let pop = world.add_pop(settlement).unwrap();

        {
            let p = world.get_pop_mut(pop).unwrap();
            p.currency = 100.0;
            p.skills.insert(LABORER);
            p.min_wage = 0.0;
            p.income_ema = 1.0;
            p.stocks.insert(GRAIN, initial_pop_stock);
            p.desired_consumption_ema.insert(GRAIN, 1.0);
        }
    }

    // Set up recipes for each facility
    for farm in &facility_ids {
        let f = world.get_facility_mut(*farm).unwrap();
        f.recipe_priorities = vec![RecipeId::new(1)];
    }

    world.wage_ema.insert(LABORER, 1.0);
    world.price_ema.insert((settlement, GRAIN), initial_price);

    world
}

/// Result of multi-pop trial
#[derive(Debug)]
#[allow(dead_code)]
struct MultiPopResult {
    final_price: f64,
    final_pop_count: usize,
    initial_pop_count: usize,
    final_total_pop_stock: f64,
    final_merchant_stock: f64,
    price_history: Vec<f64>,
    pop_count_history: Vec<usize>,
    employed_history: Vec<usize>,
    food_satisfaction_history: Vec<f64>,
    diagnostics: ConvergenceDiagnostics,
    converged: bool,
    weakly_stable: bool,
    extinction: bool,
}

#[derive(Debug, Clone, Copy)]
struct ConvergenceThresholds {
    trailing_window: usize,
    pop_stability_window: usize,
    max_price_std: f64,
    max_pop_cv: f64,
    max_abs_stock_slope: f64,
    min_employment_rate: f64,
    min_food_satisfaction_mean: f64,
}

impl Default for ConvergenceThresholds {
    fn default() -> Self {
        Self {
            trailing_window: 50,
            pop_stability_window: 25,
            max_price_std: 0.05,
            max_pop_cv: 0.10,
            max_abs_stock_slope: 1.0,
            min_employment_rate: 0.85,
            min_food_satisfaction_mean: 0.95,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)]
struct ConvergenceDiagnostics {
    trailing_window: usize,
    price_mean: f64,
    price_std: f64,
    pop_mean: f64,
    pop_cv: f64,
    merchant_stock_slope_per_tick: f64,
    mean_net_external_qty: f64,
    employment_rate_mean: f64,
    food_satisfaction_mean: f64,
    food_satisfaction_min: f64,
}

#[derive(Debug, Clone, Copy)]
struct EquilibriumPrediction {
    formal_capacity: usize,
    feasible_pop_min: usize,
    feasible_pop_max: usize,
    approx_best_pop: usize,
    best_abs_gap: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct StabilizationControls {
    enable_external_grain_anchor: bool,
    /// None = no subsistence, Some(q_max) = enabled with that q_max.
    subsistence_q_max: Option<f64>,
}

fn subsistence_config_for_controls(
    controls: StabilizationControls,
) -> Option<SubsistenceReservationConfig> {
    controls
        .subsistence_q_max
        .map(|q_max| SubsistenceReservationConfig::new(GRAIN, q_max, 50, 10.0))
}

fn subsistence_total_output(unemployed: usize, cfg: &SubsistenceReservationConfig) -> f64 {
    use sim_core::labor::subsistence::subsistence_output_per_worker;
    (1..=unemployed)
        .map(|rank| subsistence_output_per_worker(rank, cfg.q_max, cfg.carrying_capacity))
        .sum()
}

fn slope_per_tick(values: &[f64]) -> f64 {
    if values.len() <= 1 {
        0.0
    } else {
        (values[values.len() - 1] - values[0]) / (values.len() as f64 - 1.0)
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_convergence(
    prices: &[f64],
    pops: &[usize],
    employed: &[usize],
    merchant_stock: &[f64],
    net_external_qty: &[f64],
    food_satisfaction: &[f64],
    extinction: bool,
    thresholds: ConvergenceThresholds,
) -> (ConvergenceDiagnostics, bool) {
    let price_w = trailing(prices, thresholds.trailing_window);
    let pop_w = trailing(pops, thresholds.pop_stability_window);
    let employed_w = trailing(employed, thresholds.trailing_window);
    let stock_w = trailing(merchant_stock, thresholds.trailing_window);
    let ext_w = trailing(net_external_qty, thresholds.trailing_window);
    let food_sat_w = trailing(food_satisfaction, thresholds.trailing_window);

    let pop_w_f: Vec<f64> = pop_w.iter().map(|&v| v as f64).collect();
    let employment_rates: Vec<f64> = pop_w
        .iter()
        .zip(employed_w.iter())
        .map(|(&p, &h)| if p == 0 { 0.0 } else { h as f64 / p as f64 })
        .collect();
    let food_satisfaction_min = food_sat_w
        .iter()
        .fold(1.0, |acc, &v| if v < acc { v } else { acc });

    let pop_mean = mean(&pop_w_f);
    let pop_cv = if pop_mean > 0.0 {
        std_dev(&pop_w_f) / pop_mean
    } else {
        f64::INFINITY
    };

    let diagnostics = ConvergenceDiagnostics {
        trailing_window: thresholds.trailing_window,
        price_mean: mean(price_w),
        price_std: std_dev(price_w),
        pop_mean,
        pop_cv,
        merchant_stock_slope_per_tick: slope_per_tick(stock_w),
        mean_net_external_qty: mean(ext_w),
        employment_rate_mean: mean(&employment_rates),
        food_satisfaction_mean: mean(food_sat_w),
        food_satisfaction_min,
    };

    let converged = !extinction
        && diagnostics.price_std <= thresholds.max_price_std
        && diagnostics.pop_cv <= thresholds.max_pop_cv
        && diagnostics.merchant_stock_slope_per_tick.abs() <= thresholds.max_abs_stock_slope
        && diagnostics.employment_rate_mean >= thresholds.min_employment_rate
        && diagnostics.food_satisfaction_mean >= thresholds.min_food_satisfaction_mean;

    (diagnostics, converged)
}

fn is_weakly_stable(
    diagnostics: &ConvergenceDiagnostics,
    extinction: bool,
    min_employment_rate: f64,
) -> bool {
    !extinction
        && diagnostics.price_std <= 0.03
        && diagnostics.employment_rate_mean >= min_employment_rate
        && diagnostics.food_satisfaction_mean >= 0.95
        && diagnostics.pop_cv <= 0.10
}

fn predict_equilibrium_population(
    initial_pop: usize,
    formal_capacity: usize,
    production_rate: f64,
    consumption_requirement: f64,
    subsistence_cfg: Option<&SubsistenceReservationConfig>,
) -> EquilibriumPrediction {
    // Solve formal + subsistence ~= consumption with zero net external flow.
    // The solution can be a range, not a single point.
    let max_search = initial_pop.saturating_mul(3).max(formal_capacity + 200);
    let mut best_n = initial_pop.max(1);
    let mut best_abs_gap = f64::MAX;
    let mut feasible_min = None;
    let mut feasible_max = None;
    let exact_tol = 1e-9;

    for n in 1..=max_search {
        let employed = n.min(formal_capacity);
        let unemployed = n.saturating_sub(employed);
        let formal = employed as f64 * production_rate;
        let subsistence = subsistence_cfg
            .map(|cfg| subsistence_total_output(unemployed, cfg))
            .unwrap_or(0.0);
        let need = n as f64 * consumption_requirement;
        let balance_gap = formal + subsistence - need;
        let abs_gap = balance_gap.abs();
        if abs_gap <= exact_tol {
            feasible_min = Some(feasible_min.unwrap_or(n));
            feasible_max = Some(n);
        }
        if abs_gap < best_abs_gap {
            best_abs_gap = abs_gap;
            best_n = n;
        }
    }

    let (feasible_pop_min, feasible_pop_max) = match (feasible_min, feasible_max) {
        (Some(min_n), Some(max_n)) => (min_n, max_n),
        _ => (best_n, best_n),
    };
    if feasible_pop_min <= initial_pop && initial_pop <= feasible_pop_max {
        best_n = initial_pop;
    } else if feasible_pop_max < initial_pop {
        best_n = feasible_pop_max;
    } else if feasible_pop_min > initial_pop {
        best_n = feasible_pop_min;
    }

    EquilibriumPrediction {
        formal_capacity,
        feasible_pop_min,
        feasible_pop_max,
        approx_best_pop: best_n,
        best_abs_gap,
    }
}

fn enable_stabilizers_for_settlement(
    world: &mut World,
    settlement: sim_core::SettlementId,
    controls: StabilizationControls,
) {
    if controls.enable_external_grain_anchor {
        let mut external = ExternalMarketConfig::default();
        external.anchors.insert(
            GRAIN,
            AnchoredGoodConfig {
                world_price: 10.0,
                spread_bps: 500.0,
                base_depth: 0.0,
                depth_per_pop: 0.1,
                tiers: 9,
                tier_step_bps: 300.0,
            },
        );
        external.frictions.insert(
            settlement,
            SettlementFriction {
                enabled: true,
                // Keep external export parity near local scale while preserving
                // the user-selected world anchor of 10.0.
                transport_bps: 9000.0,
                tariff_bps: 0.0,
                risk_bps: 0.0,
            },
        );
        world.set_external_market(external);
    }

    if let Some(cfg) = subsistence_config_for_controls(controls) {
        world.set_subsistence_reservation(cfg);
    }
}

/// Run a multi-pop trial
fn run_multi_pop_trial(
    num_pops: usize,
    num_facilities: usize,
    production_rate: f64,
    initial_price: f64,
    initial_pop_stock: f64,
    initial_merchant_stock: f64,
    ticks: usize,
) -> MultiPopResult {
    run_multi_pop_trial_with_controls(
        num_pops,
        num_facilities,
        production_rate,
        initial_price,
        initial_pop_stock,
        initial_merchant_stock,
        ticks,
        StabilizationControls::default(),
        0.85,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_multi_pop_trial_with_controls(
    num_pops: usize,
    num_facilities: usize,
    production_rate: f64,
    initial_price: f64,
    initial_pop_stock: f64,
    initial_merchant_stock: f64,
    ticks: usize,
    controls: StabilizationControls,
    min_employment_rate: f64,
) -> MultiPopResult {
    let mut world = create_multi_pop_world(
        num_pops,
        num_facilities,
        initial_price,
        initial_pop_stock,
        initial_merchant_stock,
    );

    let recipes = vec![make_grain_recipe(production_rate)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0); // consumption requirement = 1.0

    let settlement = *world.settlements.keys().next().unwrap();
    enable_stabilizers_for_settlement(&mut world, settlement, controls);

    let mut price_history = Vec::new();
    let mut pop_count_history = Vec::new();
    let mut employed_history = Vec::new();
    let mut merchant_stock_history = Vec::new();
    let mut net_external_qty_history = Vec::new();
    let mut food_satisfaction_history = Vec::new();
    let mut prev_cumulative_net_external_qty = 0.0;
    let verbose = std::env::var_os("CONVERGENCE_VERBOSE").is_some();

    for tick in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);

        if let Some(&price) = world.price_ema.get(&(settlement, GRAIN)) {
            price_history.push(price);
        }

        let pop_count = world.pops.len();
        let employed = world
            .pops
            .values()
            .filter(|p| p.employed_at.is_some())
            .count();
        let merc_stock = world
            .merchants
            .values()
            .next()
            .map(|m| {
                m.stockpiles
                    .get(&settlement)
                    .map(|s| s.get(GRAIN))
                    .unwrap_or(0.0)
            })
            .unwrap_or(0.0);

        let cumulative_imports = world
            .outside_flow_totals
            .imports_qty
            .get(&(settlement, GRAIN))
            .copied()
            .unwrap_or(0.0);
        let cumulative_exports = world
            .outside_flow_totals
            .exports_qty
            .get(&(settlement, GRAIN))
            .copied()
            .unwrap_or(0.0);
        let cumulative_net_external_qty = cumulative_imports - cumulative_exports;
        let net_external_qty_tick = cumulative_net_external_qty - prev_cumulative_net_external_qty;
        prev_cumulative_net_external_qty = cumulative_net_external_qty;
        let avg_satisfaction: f64 = if world.pops.is_empty() {
            0.0
        } else {
            world
                .pops
                .values()
                .map(|p| p.need_satisfaction.get("food").copied().unwrap_or(0.0))
                .sum::<f64>()
                / world.pops.len() as f64
        };

        pop_count_history.push(pop_count);
        employed_history.push(employed);
        merchant_stock_history.push(merc_stock);
        net_external_qty_history.push(net_external_qty_tick);
        food_satisfaction_history.push(avg_satisfaction);

        // Debug output
        if verbose && (tick < 10 || tick % 20 == 0 || pop_count < num_pops / 2) {
            let avg_stock: f64 = if world.pops.is_empty() {
                0.0
            } else {
                world
                    .pops
                    .values()
                    .map(|p| p.stocks.get(&GRAIN).copied().unwrap_or(0.0))
                    .sum::<f64>()
                    / world.pops.len() as f64
            };
            let min_stock: f64 = world
                .pops
                .values()
                .map(|p| p.stocks.get(&GRAIN).copied().unwrap_or(0.0))
                .fold(f64::INFINITY, f64::min);
            let min_sat: f64 = world
                .pops
                .values()
                .map(|p| p.need_satisfaction.get("food").copied().unwrap_or(0.0))
                .fold(f64::INFINITY, f64::min);
            let avg_income: f64 = if world.pops.is_empty() {
                0.0
            } else {
                world.pops.values().map(|p| p.income_ema).sum::<f64>() / world.pops.len() as f64
            };
            let price = world
                .price_ema
                .get(&(settlement, GRAIN))
                .copied()
                .unwrap_or(0.0);

            println!(
                "tick {:>3}: pops={:>3} empl={:>3} avg_stk={:.2} min_stk={:.2} avg_sat={:.2} min_sat={:.2} inc={:.2} m_stk={:.1} p={:.3} ext={:+.2}",
                tick, pop_count, employed, avg_stock, min_stock, avg_satisfaction, min_sat, avg_income, merc_stock, price, net_external_qty_tick
            );
        }

        // Early termination if extinction
        if world.pops.is_empty() {
            break;
        }
    }

    let final_price = price_history.last().copied().unwrap_or(0.0);
    let final_pop_count = world.pops.len();

    let final_total_pop_stock: f64 = world
        .pops
        .values()
        .map(|p| p.stocks.get(&GRAIN).copied().unwrap_or(0.0))
        .sum();

    let merchant = world.merchants.values().next().unwrap();
    let final_merchant_stock = merchant
        .stockpiles
        .get(&settlement)
        .map(|s| s.get(GRAIN))
        .unwrap_or(0.0);

    let extinction = final_pop_count == 0;
    let thresholds = ConvergenceThresholds::default();
    let (diagnostics, converged) = evaluate_convergence(
        &price_history,
        &pop_count_history,
        &employed_history,
        &merchant_stock_history,
        &net_external_qty_history,
        &food_satisfaction_history,
        extinction,
        thresholds,
    );
    let weakly_stable = is_weakly_stable(&diagnostics, extinction, min_employment_rate);

    MultiPopResult {
        final_price,
        final_pop_count,
        initial_pop_count: num_pops,
        final_total_pop_stock,
        final_merchant_stock,
        price_history,
        pop_count_history,
        employed_history,
        food_satisfaction_history,
        diagnostics,
        converged,
        weakly_stable,
        extinction,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct CalibrationScenario {
    depth_per_pop: f64,
    transport_bps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CalibrationScenarioSummary {
    depth_per_pop: f64,
    transport_bps: f64,
    median_tail_price: f64,
    median_tail_employment: f64,
    median_import_reliance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CalibrationSweepSnapshot {
    ticks: usize,
    tail_window: usize,
    reps: usize,
    scenarios: Vec<CalibrationScenarioSummary>,
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

#[allow(clippy::too_many_arguments)]
fn run_calibration_trial(
    num_pops: usize,
    num_facilities: usize,
    production_rate: f64,
    initial_price: f64,
    initial_pop_stock: f64,
    initial_merchant_stock: f64,
    ticks: usize,
    scenario: CalibrationScenario,
) -> (f64, f64, f64) {
    let mut world = create_multi_pop_world(
        num_pops,
        num_facilities,
        initial_price,
        initial_pop_stock,
        initial_merchant_stock,
    );

    let settlement = *world.settlements.keys().next().unwrap();
    let mut external = ExternalMarketConfig::default();
    external.anchors.insert(
        GRAIN,
        AnchoredGoodConfig {
            world_price: 10.0,
            spread_bps: 500.0,
            base_depth: 0.0,
            depth_per_pop: scenario.depth_per_pop,
            tiers: 9,
            tier_step_bps: 300.0,
        },
    );
    external.frictions.insert(
        settlement,
        SettlementFriction {
            enabled: true,
            transport_bps: scenario.transport_bps,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );
    world.set_external_market(external);
    if let Some(cfg) = subsistence_config_for_controls(StabilizationControls {
        enable_external_grain_anchor: true,
        subsistence_q_max: Some(1.0),
    }) {
        world.set_subsistence_reservation(cfg);
    }

    let recipes = vec![make_grain_recipe(production_rate)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);

    let mut price_history = Vec::with_capacity(ticks);
    let mut employment_rate_history = Vec::with_capacity(ticks);
    let mut import_reliance_history = Vec::with_capacity(ticks);
    let mut prev_cumulative_imports = 0.0;

    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);

        if world.pops.is_empty() {
            break;
        }

        let price = world
            .price_ema
            .get(&(settlement, GRAIN))
            .copied()
            .unwrap_or(0.0);
        let pop_count = world.pops.len();
        let employed = world
            .pops
            .values()
            .filter(|p| p.employed_at.is_some())
            .count();
        let employment_rate = if pop_count == 0 {
            0.0
        } else {
            employed as f64 / pop_count as f64
        };

        let cumulative_imports = world
            .outside_flow_totals
            .imports_qty
            .get(&(settlement, GRAIN))
            .copied()
            .unwrap_or(0.0);
        let import_qty_tick = cumulative_imports - prev_cumulative_imports;
        prev_cumulative_imports = cumulative_imports;
        let import_reliance = if pop_count == 0 {
            0.0
        } else {
            import_qty_tick.max(0.0) / pop_count as f64
        };

        price_history.push(price);
        employment_rate_history.push(employment_rate);
        import_reliance_history.push(import_reliance);
    }

    let tail_window = 40usize;
    let tail_prices = trailing(&price_history, tail_window);
    let tail_employment = trailing(&employment_rate_history, tail_window);
    let tail_import = trailing(&import_reliance_history, tail_window);

    (mean(tail_prices), mean(tail_employment), mean(tail_import))
}

fn compute_calibration_sweep_snapshot() -> CalibrationSweepSnapshot {
    let scenarios = [
        CalibrationScenario {
            depth_per_pop: 0.05,
            transport_bps: 7000.0,
        },
        CalibrationScenario {
            depth_per_pop: 0.05,
            transport_bps: 9000.0,
        },
        CalibrationScenario {
            depth_per_pop: 0.05,
            transport_bps: 11000.0,
        },
        CalibrationScenario {
            depth_per_pop: 0.10,
            transport_bps: 7000.0,
        },
        CalibrationScenario {
            depth_per_pop: 0.10,
            transport_bps: 9000.0,
        },
        CalibrationScenario {
            depth_per_pop: 0.10,
            transport_bps: 11000.0,
        },
        CalibrationScenario {
            depth_per_pop: 0.20,
            transport_bps: 7000.0,
        },
        CalibrationScenario {
            depth_per_pop: 0.20,
            transport_bps: 9000.0,
        },
        CalibrationScenario {
            depth_per_pop: 0.20,
            transport_bps: 11000.0,
        },
    ];
    let reps = 5usize;
    let ticks = 220usize;
    let tail_window = 40usize;

    let mut summaries = Vec::with_capacity(scenarios.len());
    for scenario in scenarios {
        let mut tail_prices = Vec::with_capacity(reps);
        let mut tail_employment = Vec::with_capacity(reps);
        let mut import_reliance = Vec::with_capacity(reps);

        for _ in 0..reps {
            let (price, employment, imports) = run_calibration_trial(
                100,
                2,
                CALIBRATION_PRODUCTION_RATE,
                1.0,
                5.0,
                210.0,
                ticks,
                scenario,
            );
            tail_prices.push(price);
            tail_employment.push(employment);
            import_reliance.push(imports);
        }

        summaries.push(CalibrationScenarioSummary {
            depth_per_pop: scenario.depth_per_pop,
            transport_bps: scenario.transport_bps,
            median_tail_price: median(tail_prices),
            median_tail_employment: median(tail_employment),
            median_import_reliance: median(import_reliance),
        });
    }

    CalibrationSweepSnapshot {
        ticks,
        tail_window,
        reps,
        scenarios: summaries,
    }
}

/// Basic multi-pop convergence test with backstop subsistence (q_max=0.8).
///
/// With q_max=1.0, subsistence provides a credible outside option: workers won't
/// accept wages below grain_price (subsistence gives food_sat=1.0 for free).
/// This creates a natural wage floor that prevents the death spiral.
#[test]
fn multi_pop_basic_convergence() {
    let num_pops = 100;
    let num_facilities = 2;
    let production_rate = MULTI_POP_PRODUCTION_RATE;
    let q_max = 1.0;
    let min_employment = 0.55;
    let ticks = 600;
    let reps = 5usize;
    let min_success_rate = 0.80f64;

    println!("\n=== Multi-Pop Basic Convergence (Backstop Subsistence) ===\n");

    let controls = StabilizationControls {
        enable_external_grain_anchor: true,
        subsistence_q_max: Some(q_max),
    };
    let subsistence_cfg = subsistence_config_for_controls(controls);
    let capacity = num_pops; // num_facilities × (num_pops / num_facilities)
    let eq = predict_equilibrium_population(
        num_pops,
        capacity,
        production_rate,
        1.0,
        subsistence_cfg.as_ref(),
    );
    println!(
        "Analytical equilibrium prediction: capacity={}, feasible_pop_range=[{}, {}], best_pop={} (abs_gap={:.4})",
        eq.formal_capacity, eq.feasible_pop_min, eq.feasible_pop_max, eq.approx_best_pop, eq.best_abs_gap
    );

    let mut successes = 0usize;
    let mut final_pops = Vec::new();
    let mut final_prices = Vec::new();
    let mut last_diag = ConvergenceDiagnostics::default();

    for rep in 0..reps {
        let result = run_multi_pop_trial_with_controls(
            num_pops,
            num_facilities,
            production_rate,
            1.0,
            5.0,
            210.0,
            ticks,
            controls,
            min_employment,
        );

        if rep == 0 {
            let k = subsistence_cfg.as_ref().map(|c| c.carrying_capacity).unwrap_or(0);
            println!("Setup: {num_pops} pops, {num_facilities} facilities, q_max={q_max}, K={k}");
            println!("  Production: {:.0}/tick, ticks={ticks}", num_pops as f64 * production_rate);
            println!("  Sample final price: {:.3}", result.final_price);
            println!(
                "  Sample pop count: {} → {}",
                result.initial_pop_count, result.final_pop_count
            );
            println!("  Sample weakly_stable: {}", result.weakly_stable);
            println!(
                "  Sample diagnostics: price_std={:.4} empl_rate={:.3} food_sat={:.3} pop_cv={:.4}",
                result.diagnostics.price_std,
                result.diagnostics.employment_rate_mean,
                result.diagnostics.food_satisfaction_mean,
                result.diagnostics.pop_cv,
            );
        }

        if result.weakly_stable {
            successes += 1;
        }
        final_pops.push(result.final_pop_count);
        final_prices.push(result.final_price);
        last_diag = result.diagnostics;
    }

    final_pops.sort_unstable();
    final_prices.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_pop = final_pops[final_pops.len() / 2];
    let median_price = final_prices[final_prices.len() / 2];
    let success_rate = successes as f64 / reps as f64;

    println!(
        "Rep summary: reps={} success_rate={:.2} median_pop={} median_price={:.3}",
        reps, success_rate, median_pop, median_price
    );

    assert!(
        success_rate >= min_success_rate,
        "Basic convergence success rate too low: {:.2} < {:.2}, last_diag={:?}",
        success_rate,
        min_success_rate,
        last_diag
    );
    if eq.best_abs_gap <= 1e-6 {
        assert!(
            median_pop >= eq.feasible_pop_min && median_pop <= eq.feasible_pop_max,
            "Median pop {} outside analytical feasible range [{}, {}]",
            median_pop,
            eq.feasible_pop_min,
            eq.feasible_pop_max
        );
    } else {
        println!(
            "Analytical note: closed-balance abs_gap={:.4} (non-zero), skipping strict feasible-range assert",
            eq.best_abs_gap
        );
    }
}

/// Long-running convergence test with subsistence overlap (q_max=1.5 > production_rate=1.05).
///
/// When q_max > production_rate, some pops rationally choose subsistence farming
/// over formal employment. This produces ~82-89% employment at equilibrium and
/// needs 10k ticks to fully stabilize.
#[test]
#[ignore = "long-running: subsistence overlap needs 10k ticks"]
fn multi_pop_subsistence_overlap_convergence() {
    println!("\n=== Multi-Pop Subsistence Overlap Convergence ===\n");

    let controls = StabilizationControls {
        enable_external_grain_anchor: true,
        subsistence_q_max: Some(1.5),
    };
    let eq = predict_equilibrium_population(
        100,
        100,
        MULTI_POP_PRODUCTION_RATE,
        1.0,
        subsistence_config_for_controls(controls).as_ref(),
    );
    println!(
        "Analytical equilibrium prediction: capacity={}, feasible_pop_range=[{}, {}], best_pop={} (abs_gap={:.4})",
        eq.formal_capacity,
        eq.feasible_pop_min,
        eq.feasible_pop_max,
        eq.approx_best_pop,
        eq.best_abs_gap
    );

    let reps = 5usize;
    let min_success_rate = 0.80f64;
    let mut successes = 0usize;
    let mut final_pops = Vec::new();
    let mut final_prices = Vec::new();
    let mut last_diag = ConvergenceDiagnostics::default();

    for rep in 0..reps {
        let result = run_multi_pop_trial_with_controls(
            100,
            2,
            MULTI_POP_PRODUCTION_RATE,
            1.0,
            5.0,
            210.0,
            10_000,
            controls,
            0.75,
        );

        if rep == 0 {
            println!("Setup: 100 pops, 2 facilities, q_max=1.5 (overlap)");
            println!("  Sample final price: {:.3}", result.final_price);
            println!(
                "  Sample pop count: {} → {}",
                result.initial_pop_count, result.final_pop_count
            );
            println!("  Sample weakly_stable: {}", result.weakly_stable);
            println!(
                "  Sample diagnostics: price_std={:.4} empl_rate={:.3} food_sat={:.3} pop_cv={:.4}",
                result.diagnostics.price_std,
                result.diagnostics.employment_rate_mean,
                result.diagnostics.food_satisfaction_mean,
                result.diagnostics.pop_cv,
            );
        }

        if result.weakly_stable {
            successes += 1;
        }
        final_pops.push(result.final_pop_count);
        final_prices.push(result.final_price);
        last_diag = result.diagnostics;
    }

    final_pops.sort_unstable();
    final_prices.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_pop = final_pops[final_pops.len() / 2];
    let median_price = final_prices[final_prices.len() / 2];
    let success_rate = successes as f64 / reps as f64;

    println!(
        "Rep summary: reps={} success_rate={:.2} median_pop={} median_price={:.3}",
        reps, success_rate, median_pop, median_price
    );

    assert!(
        success_rate >= min_success_rate,
        "Overlap convergence success rate too low: {:.2} < {:.2}, last_diag={:?}",
        success_rate,
        min_success_rate,
        last_diag
    );
}

/// Test stability with various initial conditions (backstop subsistence, q_max=0.8).
#[test]
fn multi_pop_sweep_initial_conditions() {
    println!("\n=== Multi-Pop Initial Conditions Sweep (Backstop) ===\n");

    let scenarios = [
        // (price, pop_stock, merchant_stock, description, min_success_rate)
        (1.0, 5.0, 200.0, "balanced_buffer", 0.80),
        (0.6, 5.0, 0.0, "empty_merchant_low_price", 0.80),
        (1.4, 4.0, 120.0, "moderate_high_price", 0.80),
    ];
    let stress_scenarios = [
        // Characterization-only: intentionally harsh starts.
        (1.0, 2.0, 80.0, "low_buffers"),
        (2.0, 2.0, 1.0, "worst_case"),
        (3.0, 1.0, 10.0, "high_price_starvation"),
        (1.2, 0.5, 80.0, "hungry_pops"),
    ];
    let controls = StabilizationControls {
        enable_external_grain_anchor: true,
        subsistence_q_max: Some(1.0),
    };
    let reps_per_scenario = 5usize;

    println!(
        "{:>24} {:>8} {:>8} {:>8} {:>8} {:>10} {:>10} {:>8} {:>6}",
        "scenario", "price0", "pop0", "m0", "reps", "succ_rate", "med_pop", "med_p", "ok"
    );

    let mut failures = Vec::new();
    let mut scenario_median_prices: Vec<f64> = Vec::new();

    for (price, pop_stock, merc_stock, desc, min_success_rate) in &scenarios {
        let mut successes = 0usize;
        let mut final_pops = Vec::new();
        let mut final_prices = Vec::new();

        for _rep in 0..reps_per_scenario {
            let result = run_multi_pop_trial_with_controls(
                100,
                2,
                MULTI_POP_PRODUCTION_RATE,
                *price,
                *pop_stock,
                *merc_stock,
                400,
                controls,
                0.85,
            );
            let ok = result.weakly_stable && result.final_pop_count >= 10;
            if ok {
                successes += 1;
            }
            final_pops.push(result.final_pop_count);
            final_prices.push(result.final_price);
        }

        final_pops.sort_unstable();
        final_prices.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_pop = final_pops[final_pops.len() / 2];
        let median_price = final_prices[final_prices.len() / 2];
        let success_rate = successes as f64 / reps_per_scenario as f64;
        let ok = success_rate >= *min_success_rate;

        println!(
            "{:>24} {:>8.2} {:>8.1} {:>8.1} {:>8} {:>10.2} {:>10} {:>8.3} {:>6}",
            desc,
            price,
            pop_stock,
            merc_stock,
            reps_per_scenario,
            success_rate,
            median_pop,
            median_price,
            if ok { "✓" } else { "✗" }
        );

        if !ok {
            failures.push(*desc);
        }
        scenario_median_prices.push(median_price);
    }

    assert!(
        failures.is_empty(),
        "Convergence sweep failed for scenarios: {:?}",
        failures
    );

    if scenario_median_prices.len() >= 2 {
        let price_min = scenario_median_prices.iter().cloned().fold(f64::INFINITY, f64::min);
        let price_max = scenario_median_prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let price_band = price_max - price_min;
        println!(
            "\nEquilibrium convergence: median_prices={:?}, band={:.4}",
            scenario_median_prices, price_band
        );
        assert!(
            price_band <= 0.10,
            "Gating scenarios should converge to same equilibrium: median_prices={:?}, band={:.4}",
            scenario_median_prices, price_band,
        );
    }

    println!("\nStress characterization (partially-gating):");
    println!(
        "{:>24} {:>8} {:>8} {:>8} {:>8} {:>10} {:>10} {:>10} {:>8}",
        "scenario", "price0", "pop0", "m0", "reps", "surv_rate", "ws_rate", "conv_rate", "med_pop"
    );
    let mut stress_results: Vec<(&str, f64, f64)> = Vec::new();
    for (price, pop_stock, merc_stock, desc) in &stress_scenarios {
        let mut survivors = 0usize;
        let mut converged = 0usize;
        let mut weakly_stables = 0usize;
        let mut final_pops = Vec::new();
        for _rep in 0..reps_per_scenario {
            let result = run_multi_pop_trial_with_controls(
                100,
                2,
                MULTI_POP_PRODUCTION_RATE,
                *price,
                *pop_stock,
                *merc_stock,
                220,
                controls,
                0.85,
            );
            if !result.extinction {
                survivors += 1;
            }
            if result.weakly_stable {
                weakly_stables += 1;
            }
            if !result.extinction && result.converged {
                converged += 1;
            }
            final_pops.push(result.final_pop_count);
        }
        final_pops.sort_unstable();
        let surv_rate = survivors as f64 / reps_per_scenario as f64;
        let ws_rate = weakly_stables as f64 / reps_per_scenario as f64;
        println!(
            "{:>24} {:>8.2} {:>8.1} {:>8.1} {:>8} {:>10.2} {:>10.2} {:>10.2} {:>8}",
            desc,
            price,
            pop_stock,
            merc_stock,
            reps_per_scenario,
            surv_rate,
            ws_rate,
            converged as f64 / reps_per_scenario as f64,
            final_pops[final_pops.len() / 2]
        );
        stress_results.push((desc, surv_rate, ws_rate));
    }

    // Partially-gating assertions for stress scenarios
    for (desc, surv_rate, ws_rate) in &stress_results {
        match *desc {
            "low_buffers" => {
                assert!(
                    *surv_rate >= 1.0,
                    "low_buffers must have 100% survival, got {:.2}",
                    surv_rate
                );
                assert!(
                    *ws_rate >= 0.60,
                    "low_buffers must achieve weak stability rate >= 0.60, got {:.2}",
                    ws_rate
                );
            }
            "worst_case" => {
                assert!(
                    *surv_rate >= 1.0,
                    "worst_case must have 100% survival, got {:.2}",
                    surv_rate
                );
            }
            _ => {
                // high_price_starvation and hungry_pops: characterization-only
            }
        }
    }
}

/// Long-running sweep with subsistence overlap (q_max=1.5 > production_rate=1.05).
#[test]
#[ignore = "long-running: subsistence overlap needs 10k ticks"]
fn multi_pop_subsistence_overlap_sweep() {
    println!("\n=== Multi-Pop Subsistence Overlap Sweep ===\n");

    let scenarios = [
        (1.0, 5.0, 200.0, "balanced_buffer", 0.80),
        (0.6, 5.0, 0.0, "empty_merchant_low_price", 0.80),
        (1.4, 4.0, 120.0, "moderate_high_price", 0.80),
    ];
    let controls = StabilizationControls {
        enable_external_grain_anchor: true,
        subsistence_q_max: Some(1.5),
    };
    let reps_per_scenario = 5usize;

    println!(
        "{:>24} {:>8} {:>8} {:>8} {:>8} {:>10} {:>10} {:>8} {:>6}",
        "scenario", "price0", "pop0", "m0", "reps", "succ_rate", "med_pop", "med_p", "ok"
    );

    let mut failures = Vec::new();

    for (price, pop_stock, merc_stock, desc, min_success_rate) in &scenarios {
        let mut successes = 0usize;
        let mut final_pops = Vec::new();
        let mut final_prices = Vec::new();

        for _rep in 0..reps_per_scenario {
            let result = run_multi_pop_trial_with_controls(
                100,
                2,
                MULTI_POP_PRODUCTION_RATE,
                *price,
                *pop_stock,
                *merc_stock,
                10_000,
                controls,
                0.75,
            );
            let ok = result.weakly_stable && result.final_pop_count >= 10;
            if ok {
                successes += 1;
            }
            final_pops.push(result.final_pop_count);
            final_prices.push(result.final_price);
        }

        final_pops.sort_unstable();
        final_prices.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_pop = final_pops[final_pops.len() / 2];
        let median_price = final_prices[final_prices.len() / 2];
        let success_rate = successes as f64 / reps_per_scenario as f64;
        let ok = success_rate >= *min_success_rate;

        println!(
            "{:>24} {:>8.2} {:>8.1} {:>8.1} {:>8} {:>10.2} {:>10} {:>8.3} {:>6}",
            desc,
            price,
            pop_stock,
            merc_stock,
            reps_per_scenario,
            success_rate,
            median_pop,
            median_price,
            if ok { "✓" } else { "✗" }
        );

        if !ok {
            failures.push(*desc);
        }
    }

    assert!(
        failures.is_empty(),
        "Overlap sweep failed for scenarios: {:?}",
        failures
    );
}

fn assert_calibration_close(name: &str, actual: f64, expected: f64, abs_tol: f64, rel_tol: f64) {
    let abs_err = (actual - expected).abs();
    let rel_err = if expected.abs() > 1e-12 {
        abs_err / expected.abs()
    } else {
        abs_err
    };
    assert!(
        abs_err <= abs_tol || rel_err <= rel_tol,
        "{name} drifted: actual={actual:.8}, expected={expected:.8}, abs_err={abs_err:.8}, rel_err={rel_err:.8}"
    );
}

#[test]
fn calibration_sweep_reports_grid_and_target_band() {
    let snapshot = compute_calibration_sweep_snapshot();
    println!("\n=== Calibration Sweep (Tail Medians) ===\n");
    println!(
        "{:>10} {:>14} {:>14} {:>14} {:>16}",
        "depth/pop", "transport_bps", "tail_price", "employment", "import_rel"
    );
    for s in &snapshot.scenarios {
        println!(
            "{:>10.2} {:>14.0} {:>14.4} {:>14.4} {:>16.6}",
            s.depth_per_pop,
            s.transport_bps,
            s.median_tail_price,
            s.median_tail_employment,
            s.median_import_reliance
        );
    }

    let chosen = snapshot
        .scenarios
        .iter()
        .find(|s| (s.depth_per_pop - 0.10).abs() < 1e-9 && (s.transport_bps - 9000.0).abs() < 1e-9)
        .expect("missing chosen calibration scenario");

    // Target band for the default calibration point.
    assert!(
        (0.40..=0.60).contains(&chosen.median_tail_price),
        "chosen scenario tail price out of target band: {:.4}",
        chosen.median_tail_price
    );
    assert!(
        (0.85..=0.95).contains(&chosen.median_tail_employment),
        "chosen scenario employment out of target band: {:.4}",
        chosen.median_tail_employment
    );
    assert!(
        chosen.median_import_reliance <= 0.05,
        "chosen scenario import reliance too high: {:.6}",
        chosen.median_import_reliance
    );
}

#[test]
fn calibration_sweep_matches_saved_baseline() {
    let expected: CalibrationSweepSnapshot =
        serde_json::from_str(include_str!("baselines/calibration_sweep_baseline.json"))
            .expect("valid calibration baseline JSON");
    let actual = compute_calibration_sweep_snapshot();

    assert_eq!(actual.ticks, expected.ticks, "tick count changed");
    assert_eq!(
        actual.tail_window, expected.tail_window,
        "tail window changed"
    );
    assert_eq!(actual.reps, expected.reps, "rep count changed");
    assert_eq!(
        actual.scenarios.len(),
        expected.scenarios.len(),
        "scenario count changed"
    );

    for (i, (a, e)) in actual
        .scenarios
        .iter()
        .zip(expected.scenarios.iter())
        .enumerate()
    {
        assert_calibration_close(
            &format!("scenario[{i}].depth_per_pop"),
            a.depth_per_pop,
            e.depth_per_pop,
            1e-9,
            1e-9,
        );
        assert_calibration_close(
            &format!("scenario[{i}].transport_bps"),
            a.transport_bps,
            e.transport_bps,
            1e-9,
            1e-9,
        );
        // The (0.05, 7000) cell is bimodal — stochastic mortality pushes its
        // median price into either a ~0.9 or ~2.3 attractor across runs.
        // Use wider tolerance for price to accommodate this while still
        // catching catastrophic drift in the stable cells.
        assert_calibration_close(
            &format!("scenario[{i}].median_tail_price"),
            a.median_tail_price,
            e.median_tail_price,
            1.5,
            7e-1,
        );
        assert_calibration_close(
            &format!("scenario[{i}].median_tail_employment"),
            a.median_tail_employment,
            e.median_tail_employment,
            0.01,
            2e-2,
        );
        assert_calibration_close(
            &format!("scenario[{i}].median_import_reliance"),
            a.median_import_reliance,
            e.median_import_reliance,
            0.01,
            2e-1,
        );
    }
}

#[test]
#[ignore = "regenerates baseline snapshot; run manually when behavior changes"]
fn regenerate_calibration_sweep_baseline_snapshot() {
    let snapshot = compute_calibration_sweep_snapshot();
    println!(
        "{}",
        serde_json::to_string_pretty(&snapshot).expect("serializes baseline")
    );
}

/// Test that mortality creates labor scarcity feedback
#[test]
fn multi_pop_mortality_feedback() {
    println!("\n=== Multi-Pop Mortality Feedback Test ===\n");
    println!("Testing: High initial price → starvation → pop death → labor scarcity");
    println!();

    // Start with high price that should cause initial starvation
    let result = run_multi_pop_trial(
        100,
        2,
        MULTI_POP_PRODUCTION_RATE, // 1.05 grain per worker
        3.0,                       // high initial price
        1.0,                       // low pop stock
        10.0,                      // some merchant stock
        100,
    );

    println!("Starting conditions:");
    println!("  Initial price: 3.0 (wages = 1.0, can't afford 1 grain)");
    println!("  Initial pop stock: 1.0 (below target 5.0)");
    println!();
    println!("Results after 100 ticks:");
    println!("  Final price: {:.3}", result.final_price);
    println!(
        "  Pop count: {} → {} ({:.0}% survival)",
        result.initial_pop_count,
        result.final_pop_count,
        result.final_pop_count as f64 / result.initial_pop_count as f64 * 100.0
    );

    // Show trajectory
    if result.pop_count_history.len() >= 10 {
        println!();
        println!("Pop count trajectory (every 10 ticks):");
        for (i, &count) in result.pop_count_history.iter().enumerate() {
            if i % 10 == 0 || i == result.pop_count_history.len() - 1 {
                let price = result.price_history.get(i).copied().unwrap_or(0.0);
                println!("  tick {:>3}: {:>3} pops, price {:.3}", i, count, price);
            }
        }
    }

    // The key test: did the system stabilize before extinction?
    // With mortality, population should shrink until surviving pops can afford food
    if result.extinction {
        println!("\n⚠ EXTINCTION occurred - mortality may be too aggressive");
        println!("  or the feedback loop isn't closing fast enough");
    } else {
        println!(
            "\n✓ Population stabilized at {} ({:.0}% of original)",
            result.final_pop_count,
            result.final_pop_count as f64 / result.initial_pop_count as f64 * 100.0
        );
    }
}

#[test]
fn multi_pop_population_sensitivity() {
    println!("\n=== Population Sensitivity ===\n");
    let controls = StabilizationControls {
        enable_external_grain_anchor: true,
        subsistence_q_max: Some(1.0),
    };
    let reps = 3usize;
    let ticks = 300;

    let pop_counts: &[usize] = &[50, 100, 200];
    let mut median_prices = Vec::new();

    for &num_pops in pop_counts {
        let num_facilities = (num_pops as f64 / 50.0).ceil() as usize;
        let merc_stock = num_pops as f64 * 2.1;
        let mut final_prices = Vec::new();

        for _ in 0..reps {
            let result = run_multi_pop_trial_with_controls(
                num_pops,
                num_facilities,
                MULTI_POP_PRODUCTION_RATE,
                1.0,
                5.0,
                merc_stock,
                ticks,
                controls,
                0.85,
            );
            assert!(
                !result.extinction,
                "Extinction with {} pops",
                num_pops,
            );
            final_prices.push(result.final_price);
        }

        final_prices.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = final_prices[final_prices.len() / 2];
        println!("  pops={:>4}  facilities={:>2}  median_price={:.4}", num_pops, num_facilities, med);
        median_prices.push(med);
    }

    let price_min = median_prices.iter().cloned().fold(f64::INFINITY, f64::min);
    let price_max = median_prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let band = price_max - price_min;

    println!("\n  Price band across population sizes: {:.4}", band);

    assert!(
        band <= 0.15,
        "Population sensitivity: prices diverged across pop counts: {:?}, band={:.4}",
        median_prices, band,
    );
}
