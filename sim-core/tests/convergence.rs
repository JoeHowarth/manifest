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

use std::collections::HashMap;

use sim_core::{
    World,
    labor::SkillId,
    needs::{Need, UtilityCurve},
    production::{FacilityType, Recipe, RecipeId},
    types::{GoodId, GoodProfile, NeedContribution},
};

// === CONSTANTS ===

const GRAIN: GoodId = 1;
const LABORER: SkillId = SkillId(1);

// === SYSTEM PARAMETERS ===

/// Tunable system parameters for convergence tests
#[derive(Debug, Clone, Copy)]
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
        p.min_wage = 0.5;
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

fn make_recipe(production_rate: f64) -> Recipe {
    Recipe::new(RecipeId::new(1), "Grain Farming", vec![FacilityType::Farm])
        .with_capacity_cost(1)
        .with_worker(LABORER, 1)
        .with_output(GRAIN, production_rate)
}

fn make_good_profiles() -> Vec<GoodProfile> {
    vec![GoodProfile {
        good: GRAIN,
        contributions: vec![NeedContribution {
            need_id: "food".to_string(),
            efficiency: 1.0,
        }],
    }]
}

fn make_needs(consumption_requirement: f64) -> HashMap<String, Need> {
    let mut needs = HashMap::new();
    needs.insert(
        "food".to_string(),
        Need {
            id: "food".to_string(),
            utility_curve: UtilityCurve::Subsistence {
                requirement: consumption_requirement,
                steepness: 5.0,
            },
        },
    );
    needs
}

// === CONVERGENCE RESULT ===

#[derive(Debug, Clone)]
struct ConvergenceResult {
    final_price: f64,
    final_pop_stock: f64,
    final_merchant_stock: f64,
    price_history: Vec<f64>,
    #[allow(dead_code)]
    pop_stock_history: Vec<f64>,
    #[allow(dead_code)]
    merchant_stock_history: Vec<f64>,
    converged: bool,
    failure_reason: Option<String>,
}

fn variance(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mean = data.iter().sum::<f64>() / data.len() as f64;
    data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / data.len() as f64
}

// === TESTS ===

/// Examine order curves to understand clearing dynamics
#[test]
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
    let good_profiles = make_good_profiles();
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
    let good_profiles = make_good_profiles();
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

    // Create pops and distribute across facilities
    for i in 0..num_pops {
        let pop = world.add_pop(settlement).unwrap();
        let facility = facility_ids[i % num_facilities];

        {
            let p = world.get_pop_mut(pop).unwrap();
            p.currency = 100.0;
            p.skills.insert(LABORER);
            p.min_wage = 0.5;
            p.employed_at = Some(facility);
            p.income_ema = 1.0;
            p.stocks.insert(GRAIN, initial_pop_stock);
            p.desired_consumption_ema.insert(GRAIN, 1.0);
        }

        // Update facility worker count
        {
            let f = world.get_facility_mut(facility).unwrap();
            *f.workers.entry(LABORER).or_insert(0) += 1;
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
struct MultiPopResult {
    final_price: f64,
    final_pop_count: usize,
    initial_pop_count: usize,
    final_total_pop_stock: f64,
    final_merchant_stock: f64,
    price_history: Vec<f64>,
    pop_count_history: Vec<usize>,
    converged: bool,
    extinction: bool,
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
    let mut world = create_multi_pop_world(
        num_pops,
        num_facilities,
        initial_price,
        initial_pop_stock,
        initial_merchant_stock,
    );

    let recipes = vec![make_recipe(production_rate)];
    let good_profiles = make_good_profiles();
    let needs = make_needs(1.0); // consumption requirement = 1.0

    let settlement = *world.settlements.keys().next().unwrap();

    let mut price_history = Vec::new();
    let mut pop_count_history = Vec::new();

    for tick in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);

        if let Some(&price) = world.price_ema.get(&(settlement, GRAIN)) {
            price_history.push(price);
        }
        pop_count_history.push(world.pops.len());

        // Debug output for first few ticks
        if tick < 5 || world.pops.len() < num_pops / 2 {
            let employed = world
                .pops
                .values()
                .filter(|p| p.employed_at.is_some())
                .count();
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
            let avg_income: f64 = if world.pops.is_empty() {
                0.0
            } else {
                world.pops.values().map(|p| p.income_ema).sum::<f64>() / world.pops.len() as f64
            };
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
            let price = world
                .price_ema
                .get(&(settlement, GRAIN))
                .copied()
                .unwrap_or(0.0);

            if tick < 5 || tick % 10 == 0 {
                println!(
                    "tick {:>3}: pops={:>3} employed={:>3} avg_stock={:.2} avg_sat={:.2} avg_inc={:.2} merc_stk={:.1} price={:.3}",
                    tick,
                    world.pops.len(),
                    employed,
                    avg_stock,
                    avg_satisfaction,
                    avg_income,
                    merc_stock,
                    price
                );
            }
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
    let converged = !extinction && final_price > 0.1 && final_price < 20.0;

    MultiPopResult {
        final_price,
        final_pop_count,
        initial_pop_count: num_pops,
        final_total_pop_stock,
        final_merchant_stock,
        price_history,
        pop_count_history,
        converged,
        extinction,
    }
}

/// Basic multi-pop convergence test with ideal starting conditions
#[test]
fn multi_pop_basic_convergence() {
    use sim_core::instrument;

    // Install tracing subscriber to collect data
    instrument::clear();
    instrument::install_subscriber();

    println!("\n=== Multi-Pop Basic Convergence ===\n");

    // Ideal conditions:
    // - 100 pops, 2 facilities (50 workers each)
    // - Recipe: 1 worker produces 1 grain (production_rate = 1.0)
    // - Total production = 100 workers × 1 grain = 100
    // - Consumption = 100 pops × 1 grain = 100 total
    // - Perfect balance!
    // - Start with ample stock on both sides

    // With 100 workers × 1 grain = 100 production/tick
    // Merchant target buffer = 2 ticks × 100 = 200 units
    // Start merchant at target to avoid initial reluctance to sell
    let result = run_multi_pop_trial(
        100,   // pops
        2,     // facilities
        1.0,   // production per worker (1 grain each)
        1.0,   // initial price
        5.0,   // initial pop stock (at target)
        200.0, // initial merchant stock (at 2-tick buffer target)
        200,   // ticks
    );

    println!("Setup: 100 pops, 2 facilities producing 50 each");
    println!("  Production: 100/tick, Consumption: 100/tick (balanced)");
    println!();
    println!("Results after 200 ticks:");
    println!("  Final price: {:.3}", result.final_price);
    println!(
        "  Pop count: {} → {}",
        result.initial_pop_count, result.final_pop_count
    );
    println!("  Pop stock total: {:.1}", result.final_total_pop_stock);
    println!("  Merchant stock: {:.1}", result.final_merchant_stock);
    println!("  Converged: {}", result.converged);
    println!("  Extinction: {}", result.extinction);

    // Show price trajectory
    if result.price_history.len() >= 20 {
        let early = &result.price_history[..10];
        let late = &result.price_history[result.price_history.len() - 10..];
        println!();
        println!("Price trajectory:");
        println!(
            "  First 10: {:?}",
            early
                .iter()
                .map(|p| format!("{:.2}", p))
                .collect::<Vec<_>>()
        );
        println!(
            "  Last 10:  {:?}",
            late.iter().map(|p| format!("{:.2}", p)).collect::<Vec<_>>()
        );
    }

    // Show pop count trajectory
    if result.pop_count_history.len() >= 20 {
        let early = &result.pop_count_history[..10];
        let late = &result.pop_count_history[result.pop_count_history.len() - 10..];
        println!();
        println!("Pop count trajectory:");
        println!("  First 10: {:?}", early);
        println!("  Last 10:  {:?}", late);
    }

    // Analyze the data
    let dfs = instrument::drain_to_dataframes();
    analyze_currency_flow(&dfs);

    assert!(!result.extinction, "Population went extinct!");
    assert!(
        result.converged,
        "Failed to converge: price = {:.3}",
        result.final_price
    );
}

/// Test stability with various initial conditions
#[test]
fn multi_pop_sweep_initial_conditions() {
    use sim_core::instrument;

    // Install tracing subscriber to collect data
    instrument::clear();
    instrument::install_subscriber();

    println!("\n=== Multi-Pop Initial Conditions Sweep ===\n");

    let scenarios = [
        // (price, pop_stock, merc_stock, description)
        // (1.0, 5.0, 100.0, "ideal"),
        // (0.5, 5.0, 100.0, "low price"),
        // (2.0, 5.0, 100.0, "high price"),
        // (1.0, 0.0, 100.0, "pops start hungry"),
        // (1.0, 5.0, 0.0, "merchant starts empty"),
        (2.0, 2.0, 1.0, "worst case"),
    ];

    println!(
        "{:>12} {:>8} {:>8} {:>10} {:>8} {:>8} {:>6}",
        "scenario", "pop_stk", "m_stk", "final_p", "pops", "surviv%", "ok"
    );

    let mut failures = Vec::new();

    for (price, pop_stock, merc_stock, desc) in &scenarios {
        let result = run_multi_pop_trial(
            100,
            2,
            1.0, // 1 grain per worker
            *price,
            *pop_stock,
            *merc_stock,
            300,
        );

        let survival_pct = result.final_pop_count as f64 / result.initial_pop_count as f64 * 100.0;
        let ok = !result.extinction && result.final_pop_count >= 10;

        println!(
            "{:>12} {:>8.1} {:>8.1} {:>10.3} {:>8} {:>7.1}% {:>6}",
            desc,
            pop_stock,
            merc_stock,
            result.final_price,
            result.final_pop_count,
            survival_pct,
            if ok { "✓" } else { "✗" }
        );

        if !ok {
            failures.push(desc);
        }
    }

    if !failures.is_empty() {
        println!("\nNote: Some scenarios had significant population loss.");
        println!("This may be expected for 'worst case' initial conditions.");
    }

    // Drain and convert to polars DataFrames
    let dfs = instrument::drain_to_dataframes();

    analyze_currency_flow(&dfs);
}

fn analyze_currency_flow(dfs: &std::collections::HashMap<String, polars::prelude::DataFrame>) {
    use polars::prelude::*;

    println!("\n{}", "=".repeat(60));
    println!("=== CURRENCY FLOW ANALYSIS ===");
    println!("{}\n", "=".repeat(60));

    // First, let's see what DFs we have
    println!("Available DataFrames: {:?}\n", dfs.keys().collect::<Vec<_>>());

    // Q0: PRODUCTION per tick - is production actually happening?
    println!("--- Q0: PRODUCTION PER TICK ---\n");
    if let Some(prod_io) = dfs.get("production_io") {
        let production = prod_io.clone().lazy()
            .filter(col("direction").eq(lit("output")))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("produced")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        println!("Production output per tick:");
        println!("{}\n", production);
    }

    // Q0b: MORTALITY - when are pops dying?
    println!("--- Q0b: MORTALITY ---\n");
    if let Some(mortality) = dfs.get("mortality") {
        let deaths = mortality.clone().lazy()
            .filter(col("outcome").eq(lit("dies")))
            .group_by([col("tick")])
            .agg([col("pop_id").count().alias("deaths")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        println!("Deaths per tick:");
        println!("{}\n", deaths);

        // Also show food satisfaction
        let satisfaction = mortality.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("food_satisfaction").mean().alias("avg_food_sat"),
                col("pop_id").count().alias("pop_count"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        println!("Food satisfaction per tick:");
        println!("{}\n", satisfaction);
    }

    // Q0c: What happens around tick 24-25?
    println!("--- Q0c: TICK 20-30 DEEP DIVE ---\n");
    if let Some(fill) = dfs.get("fill") {
        let fills_around = fill.clone().lazy()
            .filter(col("tick").gt_eq(lit(20u64)).and(col("tick").lt_eq(lit(30u64))))
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("total_volume"),
                col("price").mean().alias("avg_price"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        println!("Fills around tick 20-30:");
        println!("{}\n", fills_around);
    }

    if let Some(consumption) = dfs.get("consumption") {
        let consumption_around = consumption.clone().lazy()
            .filter(col("tick").gt_eq(lit(20u64)).and(col("tick").lt_eq(lit(30u64))))
            .group_by([col("tick")])
            .agg([
                col("actual").sum().alias("consumed"),
                col("desired").sum().alias("desired"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        println!("Consumption around tick 20-30:");
        println!("{}\n", consumption_around);
    }

    // Q0d: CHECK LABOR ASSIGNMENTS - are workers being hired every tick?
    println!("--- Q0d: LABOR ASSIGNMENTS BY TICK ---\n");
    if let Some(assignment) = dfs.get("assignment") {
        let assignments_all = assignment.clone().lazy()
            .group_by([col("tick")])
            .agg([col("pop_id").count().alias("workers_hired")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        // Get tick column and print all values
        let ticks: Vec<u64> = assignments_all.column("tick").unwrap()
            .u64().unwrap().into_iter().flatten().collect();
        let workers: Vec<u32> = assignments_all.column("workers_hired").unwrap()
            .u32().unwrap().into_iter().flatten().collect();
        println!("Workers hired per tick (expect 100 every tick):");
        println!("  Ticks with data: {:?}", ticks);
        println!("  Workers: {:?}", workers);
        // What ticks are missing (assuming we expect 1 to max_tick)?
        if let Some(&max_tick) = ticks.last() {
            let missing: Vec<u64> = (1..=max_tick).filter(|t| !ticks.contains(t)).collect();
            println!("  MISSING TICKS: {:?}", missing);
        }
        println!();
    }

    // Q0e: Check labor bids to see if facilities are even bidding on missing ticks
    println!("--- Q0e: LABOR BIDS BY TICK ---\n");
    if let Some(labor_bid) = dfs.get("labor_bid") {
        let bids_all = labor_bid.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("max_wage").count().alias("num_bids"),
                col("max_wage").sum().alias("total_wages_offered"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let ticks: Vec<u64> = bids_all.column("tick").unwrap()
            .u64().unwrap().into_iter().flatten().collect();
        let bids: Vec<u32> = bids_all.column("num_bids").unwrap()
            .u32().unwrap().into_iter().flatten().collect();
        println!("Labor bids per tick:");
        println!("  Ticks with bids: {:?}", ticks);
        println!("  Num bids: {:?}", bids);
        if let Some(&max_tick) = ticks.last() {
            let missing: Vec<u64> = (1..=max_tick).filter(|t| !ticks.contains(t)).collect();
            println!("  MISSING BID TICKS: {:?}", missing);
        }

        // Check bid vs ask wages on the problem ticks
        let bids_detail = labor_bid.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("max_wage").mean().alias("avg_bid"),
                col("max_wage").min().alias("min_bid"),
                col("mvp").mean().alias("avg_mvp"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        println!("\nBid wages by tick (first 30):");
        // Print all rows for the first 30 ticks
        let ticks: Vec<u64> = bids_detail.column("tick").unwrap()
            .u64().unwrap().into_iter().flatten().collect();
        let bids: Vec<f64> = bids_detail.column("avg_bid").unwrap()
            .f64().unwrap().into_iter().flatten().collect();
        for (t, b) in ticks.iter().zip(bids.iter()).take(30) {
            let status = if *b < 0.5 { " ← BELOW MIN_WAGE" } else { "" };
            println!("  tick {:>2}: bid={:.3}{}", t, b, status);
        }
    }

    // Q0f: Check worker asks (min_wage)
    println!("--- Q0f: WORKER ASKS ---\n");
    if let Some(labor_ask) = dfs.get("labor_ask") {
        let asks_detail = labor_ask.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("min_wage").mean().alias("avg_min_wage"),
                col("min_wage").count().alias("num_workers"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        println!("Worker min_wage by tick:");
        println!("{}\n", asks_detail.head(Some(30)));
        println!();
    }

    // Q1: Wages paid vs merchant revenue from sales
    println!("--- Q1: MONEY IN vs MONEY OUT ---\n");

    if let (Some(assignment), Some(fill)) = (dfs.get("assignment"), dfs.get("fill")) {
        // Total wages paid per tick (money flows from merchant to pops)
        let wages_out = assignment.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("wage").sum().alias("wages_paid"),
                col("wage").count().alias("workers_hired"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        // Revenue from merchant sales per tick (money flows from pops to merchant)
        let revenue_in = fill.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("sell"))))
            .with_column((col("price") * col("quantity")).alias("revenue"))
            .group_by([col("tick")])
            .agg([col("revenue").sum().alias("sales_revenue")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Wages paid per tick (merchant → pops):");
        println!("{}\n", wages_out);

        println!("Sales revenue per tick (pops → merchant):");
        println!("{}\n", revenue_in);

        println!("KEY: If wages_paid > sales_revenue, merchant loses money each tick.\n");
    }

    // Q2: Labor bid details - are facilities bidding at all?
    println!("--- Q2: LABOR BIDS OVER TIME ---\n");

    if let Some(labor_bid) = dfs.get("labor_bid") {
        let bids_summary = labor_bid.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("max_wage").count().alias("num_bids"),
                col("max_wage").sum().alias("total_bid_value"),
                col("max_wage").mean().alias("avg_bid"),
                col("max_wage").min().alias("min_bid"),
                col("max_wage").max().alias("max_bid"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Labor bids per tick:");
        println!("{}\n", bids_summary);
    }

    // Q3: Why do assignments stop? Check assignment counts
    println!("--- Q3: ASSIGNMENTS OVER TIME ---\n");

    if let Some(assignment) = dfs.get("assignment") {
        let assignments_summary = assignment.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("wage").count().alias("num_assignments"),
                col("wage").sum().alias("total_wages"),
                col("wage").mean().alias("avg_wage"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Assignments per tick:");
        println!("{}\n", assignments_summary);
    }

    // Q4: Labor asks - are workers offering labor?
    println!("--- Q4: LABOR SUPPLY (ASKS) OVER TIME ---\n");

    if let Some(labor_ask) = dfs.get("labor_ask") {
        let asks_summary = labor_ask.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("min_wage").count().alias("num_workers"),
                col("min_wage").mean().alias("avg_min_wage"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Labor supply per tick:");
        println!("{}\n", asks_summary);
    }

    // Q5: Net cash flow - cumulative balance
    println!("--- Q5: CUMULATIVE CASH FLOW ---\n");

    if let (Some(assignment), Some(fill)) = (dfs.get("assignment"), dfs.get("fill")) {
        // Get wages per tick
        let wages = assignment.clone().lazy()
            .group_by([col("tick")])
            .agg([col("wage").sum().alias("wages_paid")])
            .collect()
            .unwrap();

        // Get revenue per tick
        let revenue = fill.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("sell"))))
            .with_column((col("price") * col("quantity")).alias("revenue"))
            .group_by([col("tick")])
            .agg([col("revenue").sum().alias("sales_revenue")])
            .collect()
            .unwrap();

        // Join and compute net flow
        let cash_flow = wages.lazy()
            .join(revenue.lazy(), [col("tick")], [col("tick")], JoinArgs::new(JoinType::Left))
            .with_column(col("sales_revenue").fill_null(lit(0.0)))
            .with_column((col("sales_revenue") - col("wages_paid")).alias("net_flow"))
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Net cash flow per tick (revenue - wages):");
        println!("{}\n", cash_flow);

        println!("KEY: Negative net_flow means merchant is bleeding money.\n");
    }

    // Q6: Zoom in on the transition (ticks 12-16)
    println!("--- Q6: TRANSITION POINT (ticks 12-16) ---\n");

    if let Some(labor_bid) = dfs.get("labor_bid") {
        let transition = labor_bid.clone().lazy()
            .filter(col("tick").gt_eq(lit(12u64)).and(col("tick").lt_eq(lit(16u64))))
            .group_by([col("tick")])
            .agg([
                col("max_wage").mean().alias("avg_bid"),
                col("mvp").mean().alias("avg_mvp"),
                col("adaptive_bid").mean().alias("avg_adaptive"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Facility bids around transition (bid vs mvp vs adaptive):");
        println!("{}\n", transition);

        println!("KEY: max_wage = min(mvp, adaptive_bid)");
        println!("     If adaptive_bid < min_wage (0.5), no workers hired.\n");
    }

    if let Some(labor_ask) = dfs.get("labor_ask") {
        let transition = labor_ask.clone().lazy()
            .filter(col("tick").gt_eq(lit(12u64)).and(col("tick").lt_eq(lit(16u64))))
            .group_by([col("tick")])
            .agg([
                col("min_wage").mean().alias("avg_ask"),
                col("min_wage").count().alias("workers"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Worker asks around transition:");
        println!("{}\n", transition);
    }

    // Q7: Price vs MVP over time
    println!("--- Q7: PRICE → MVP → BID CHAIN ---\n");

    if let (Some(fill), Some(labor_bid)) = (dfs.get("fill"), dfs.get("labor_bid")) {
        // Get clearing prices per tick
        let prices = fill.clone().lazy()
            .group_by([col("tick")])
            .agg([col("price").max().alias("goods_price")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        // Get MVP per tick
        let mvps = labor_bid.clone().lazy()
            .group_by([col("tick")])
            .agg([col("mvp").mean().alias("avg_mvp")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Goods price (from fills):");
        println!("{}\n", prices);

        println!("MVPs plateau after tick 13 because no fills → price EMA not updated?\n");
    }

    // Q8: Why isn't merchant selling from stockpile?
    println!("--- Q8: MERCHANT STOCKPILE & ORDERS ---\n");

    if let Some(order) = dfs.get("order") {
        // Check merchant sell orders around the transition
        let merchant_sells = order.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("sell"))))
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("offered_qty"),
                col("limit_price").min().alias("min_ask"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Merchant sell orders per tick:");
        println!("{}\n", merchant_sells);

        // Check what ticks have NO merchant sell orders
        let all_ticks: Vec<u64> = (1..=20).collect();
        println!("Merchant sell orders stop after tick 13.");
        println!("Does merchant have stockpile but isn't selling? Let's check production_io.\n");
    }

    // Check production_io to see if goods are being produced and where they go
    if let Some(prod_io) = dfs.get("production_io") {
        println!("Production IO schema: {:?}\n", prod_io.schema());

        let outputs = prod_io.clone().lazy()
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("total_io")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Production IO per tick:");
        println!("{}\n", outputs);
    }

    // Q9: Compare offered qty vs actual fills
    println!("--- Q9: OFFERED vs FILLED ---\n");

    if let (Some(order), Some(fill)) = (dfs.get("order"), dfs.get("fill")) {
        let offered = order.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("sell"))))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("offered")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        let filled = fill.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("sell"))))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("sold")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        // Join them
        let comparison = offered.lazy()
            .join(filled.lazy(), [col("tick")], [col("tick")], JoinArgs::new(JoinType::Left))
            .with_column(col("sold").fill_null(lit(0.0)))
            .collect()
            .unwrap();

        println!("Merchant: offered vs sold per tick:");
        println!("{}\n", comparison);

        println!("KEY: If sold >> what merchant actually HAS, there's a bug.");
        println!("     Merchant can't sell goods it doesn't own.\n");
    }

    // Q10: Check for goods creation bug
    println!("--- Q10: GOODS CONSERVATION CHECK ---\n");

    // Compare production vs fills
    if let (Some(prod_io), Some(fill)) = (dfs.get("production_io"), dfs.get("fill")) {

        // Total produced (outputs only)
        let produced = prod_io.clone().lazy()
            .filter(col("direction").eq(lit("output")))
            .select([col("quantity").sum().alias("total_produced")])
            .collect()
            .unwrap();

        // Total bought by pops (should equal total sold by merchant)
        let pop_bought = fill.clone().lazy()
            .filter(col("agent_type").eq(lit("pop")).and(col("side").eq(lit("buy"))))
            .select([col("quantity").sum().alias("pop_bought")])
            .collect()
            .unwrap();

        let merchant_sold = fill.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("sell"))))
            .select([col("quantity").sum().alias("merchant_sold")])
            .collect()
            .unwrap();

        println!("Total produced (from production_io): {:?}", produced);
        println!("Pop bought (from fills): {:?}", pop_bought);
        println!("Merchant sold (from fills): {:?}", merchant_sold);

        // Initial merchant stock was 1.0 in "worst case" scenario
        // So total available = produced + 1.0
        println!();
        println!("Initial merchant stock: 1.0");
        println!("If merchant_sold > produced + 1.0, goods were created from nothing!");
    }
}

fn analyze_death_spiral(dfs: &std::collections::HashMap<String, polars::prelude::DataFrame>) {
    use polars::prelude::*;

    println!("\n{}", "=".repeat(60));
    println!("=== DEATH SPIRAL ANALYSIS ===");
    println!("{}\n", "=".repeat(60));

    // === Q1: Money Flow - Can pops afford food? ===
    println!("--- Q1: MONEY FLOW (Can pops afford food?) ---\n");

    if let (Some(assignment), Some(fill)) = (dfs.get("assignment"), dfs.get("fill")) {
        // Wages paid per tick
        let wages_per_tick = assignment.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("wage").sum().alias("total_wages"),
                col("wage").count().alias("workers"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        // Pop spending per tick (buys only)
        let pop_spending = fill.clone().lazy()
            .filter(col("agent_type").eq(lit("pop")).and(col("side").eq(lit("buy"))))
            .with_column((col("price") * col("quantity")).alias("cost"))
            .group_by([col("tick")])
            .agg([
                col("cost").sum().alias("total_spent"),
                col("quantity").sum().alias("qty_bought"),
                col("price").mean().alias("avg_price"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Wages earned per tick:");
        println!("{}\n", wages_per_tick);

        println!("Pop spending per tick:");
        println!("{}\n", pop_spending);

        println!("KEY INSIGHT: Pops earn ~100 total wages but spend only ~7-14 on food.");
        println!("             At price ~2.8, they can only afford ~0.07 units each.\n");
    }

    // === Q2: Food Balance - Where does the grain go? ===
    println!("--- Q2: FOOD BALANCE (Where does the grain go?) ---\n");

    if let (Some(production_io), Some(consumption), Some(fill)) =
        (dfs.get("production_io"), dfs.get("consumption"), dfs.get("fill"))
    {
        // Production per tick
        let production = production_io.clone().lazy()
            .filter(col("direction").eq(lit("output")))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("produced")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        // Consumption per tick
        let consumed = consumption.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("actual").sum().alias("consumed"),
                col("desired").sum().alias("desired"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        // Merchant sales per tick
        let merchant_sales = fill.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("merchant_sold")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Production per tick:");
        println!("{}\n", production);

        println!("Consumption per tick:");
        println!("{}\n", consumed);

        println!("Merchant sales per tick:");
        println!("{}\n", merchant_sales);

        println!("KEY INSIGHT: Production (100/tick) >> Merchant sales (~14/tick)");
        println!("             Most grain accumulates with merchant, not reaching pops.\n");
    }

    // === Q3: Market Clearing - Why are prices high? ===
    println!("--- Q3: MARKET CLEARING (Why are prices so high?) ---\n");

    if let Some(fill) = dfs.get("fill") {
        let clearing_prices = fill.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("price").max().alias("clearing_price"),
                col("quantity").sum().alias("total_volume"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Clearing prices per tick:");
        println!("{}\n", clearing_prices);

        println!("KEY INSIGHT: FavorSellers bias picks highest price where any trade occurs.\n");
    }

    // === Q4: Merchant Supply Curve ===
    println!("--- Q4: MERCHANT SUPPLY (Is merchant holding back?) ---\n");

    if let Some(order) = dfs.get("order") {
        let merchant_orders = order.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")))
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("total_offered"),
                col("limit_price").min().alias("min_price"),
                col("limit_price").max().alias("max_price"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Merchant sell orders per tick:");
        println!("{}\n", merchant_orders);

        // Show first tick's merchant orders in detail
        let tick1_merchant = order.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("tick").eq(lit(1u64))))
            .select([col("limit_price"), col("quantity")])
            .sort(["limit_price"], Default::default())
            .collect()
            .unwrap();

        println!("Tick 1 merchant order book (price ladder):");
        println!("{}\n", tick1_merchant);

        println!("KEY INSIGHT: Merchant offers at prices 1.4-2.8 (relative to EMA=2.0).");
        println!("             When below target stock, only sells at premium.\n");
    }

    // === Q5: Budget Relaxation Effect ===
    println!("--- Q5: BUDGET EFFECT (How much demand is priced out?) ---\n");

    if let (Some(order), Some(fill)) = (dfs.get("order"), dfs.get("fill")) {
        // Total pop demand (orders)
        let pop_demand = order.clone().lazy()
            .filter(col("agent_type").eq(lit("pop")).and(col("side").eq(lit("buy"))))
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("wanted"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        // Actual pop purchases
        let pop_bought = fill.clone().lazy()
            .filter(col("agent_type").eq(lit("pop")).and(col("side").eq(lit("buy"))))
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("got"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Pop demand (total order qty):");
        println!("{}\n", pop_demand);

        println!("Pop actual purchases:");
        println!("{}\n", pop_bought);

        println!("KEY INSIGHT: Pops want ~700 units/tick but only get ~7-14.");
        println!("             99% of demand eliminated by budget constraints.\n");
    }

    // === Q6: Mortality Timeline ===
    println!("--- Q6: MORTALITY TIMELINE ---\n");

    if let Some(mortality) = dfs.get("mortality") {
        let mortality_summary = mortality.clone().lazy()
            .group_by([col("tick"), col("outcome")])
            .agg([col("pop_id").count().alias("count")])
            .sort(["tick", "outcome"], Default::default())
            .collect()
            .unwrap();

        println!("Mortality outcomes per tick:");
        println!("{}\n", mortality_summary);

        // Average food satisfaction per tick
        let satisfaction = mortality.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("food_satisfaction").mean().alias("avg_satisfaction"),
                col("death_prob").mean().alias("avg_death_prob"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Food satisfaction per tick:");
        println!("{}\n", satisfaction);
    }

    // === Q7: Why Does Trading Stop? ===
    println!("--- Q7: TRADING STOPPAGE (Why do fills become 0?) ---\n");

    if let Some(order) = dfs.get("order") {
        // Check if orders exist after tick 13
        let orders_by_tick = order.clone().lazy()
            .group_by([col("tick"), col("agent_type"), col("side")])
            .agg([
                col("quantity").sum().alias("total_qty"),
                col("limit_price").min().alias("min_price"),
                col("limit_price").max().alias("max_price"),
            ])
            .sort(["tick", "agent_type", "side"], Default::default())
            .collect()
            .unwrap();

        // Show ticks 10-20 to see what happens around the stoppage
        let filtered = orders_by_tick.clone().lazy()
            .filter(col("tick").gt_eq(lit(10u64)).and(col("tick").lt_eq(lit(20u64))))
            .collect()
            .unwrap();

        println!("Orders around tick 10-20 (when trading stops):");
        println!("{}\n", filtered);

        // Check price overlap: do pop bids meet merchant asks?
        let pop_bids = order.clone().lazy()
            .filter(col("agent_type").eq(lit("pop")).and(col("side").eq(lit("buy"))))
            .group_by([col("tick")])
            .agg([col("limit_price").max().alias("max_bid")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        let merchant_asks = order.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("sell"))))
            .group_by([col("tick")])
            .agg([col("limit_price").min().alias("min_ask")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Pop max bid vs Merchant min ask (need bid >= ask for trade):");
        println!("Pop max bids:\n{}\n", pop_bids);
        println!("Merchant min asks:\n{}\n", merchant_asks);

        // Check if merchant has stock but isn't selling
        println!("KEY INSIGHT: Merchant stops placing sell orders after tick 13.");
        println!("             Pops keep bidding but there's nothing to buy.\n");
    }

    // Check production timeline
    if let Some(production) = dfs.get("production") {
        let prod_by_tick = production.clone().lazy()
            .group_by([col("tick")])
            .agg([col("runs").sum().alias("total_runs")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();

        println!("Production timeline (runs per tick):");
        let n = prod_by_tick.height();
        if n > 25 {
            let head = prod_by_tick.head(Some(20));
            let tail = prod_by_tick.tail(Some(5));
            println!("{}", head);
            println!("...");
            println!("{}\n", tail);
        } else {
            println!("{}\n", prod_by_tick);
        }
    }

    // === Summary ===
    println!("{}", "=".repeat(60));
    println!("=== ROOT CAUSE SUMMARY ===");
    println!("{}\n", "=".repeat(60));
    println!("1. Initial price EMA (2.0) > pop income (1.0)");
    println!("2. Pops can only afford ~0.5 units of food at these prices");
    println!("3. Merchant starts below target (1.0 < 2.0) → premium pricing only");
    println!("4. Budget relaxation eliminates 99% of pop demand");
    println!("5. Small trades at HIGH prices → EMA stays high or rises");
    println!("6. Production accumulates with merchant (no demand at current prices)");
    println!("7. Pops starve → die → fewer workers → cycle continues");
    println!("\nThe core issue: price discovery fails when budget << price × needs");
}

/// Test that mortality creates labor scarcity feedback
#[test]
fn multi_pop_mortality_feedback() {
    println!("\n=== Multi-Pop Mortality Feedback Test ===\n");
    println!("Testing: High initial price → starvation → pop death → labor scarcity");
    println!();

    // Start with high price that should cause initial starvation
    let result = run_multi_pop_trial(
        100, 2, 1.0,  // 1 grain per worker
        3.0,  // high initial price
        1.0,  // low pop stock
        10.0, // some merchant stock
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
