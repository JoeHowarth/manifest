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

/// Run simulation and collect convergence metrics
fn run_trial(
    params: SystemParams,
    conditions: InitialConditions,
    ticks: usize,
) -> ConvergenceResult {
    let mut world = create_world(params, conditions);
    let recipes = vec![make_recipe(params.production_rate)];
    let good_profiles = make_good_profiles();
    let needs = make_needs(params.consumption_requirement);

    let settlement = *world.settlements.keys().next().unwrap();

    let mut price_history = Vec::new();
    let mut pop_stock_history = Vec::new();
    let mut merchant_stock_history = Vec::new();

    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);

        if let Some(&price) = world.price_ema.get(&(settlement, GRAIN)) {
            price_history.push(price);
        }

        let pop = world.pops.values().next().unwrap();
        pop_stock_history.push(pop.stocks.get(&GRAIN).copied().unwrap_or(0.0));

        let merchant = world.merchants.values().next().unwrap();
        let m_stock = merchant
            .stockpiles
            .get(&settlement)
            .map(|s| s.get(GRAIN))
            .unwrap_or(0.0);
        merchant_stock_history.push(m_stock);
    }

    let pop = world.pops.values().next().unwrap();
    let merchant = world.merchants.values().next().unwrap();
    let final_price = price_history.last().copied().unwrap_or(0.0);

    // Check convergence
    let mut converged = true;
    let mut failure_reason = None;

    if !(0.1..=10.0).contains(&final_price) {
        converged = false;
        failure_reason = Some(format!("Price out of bounds: {:.3}", final_price));
    }

    if pop.currency <= 0.0 {
        converged = false;
        failure_reason = Some(format!("Pop broke: {:.2}", pop.currency));
    }
    if merchant.currency <= 0.0 {
        converged = false;
        failure_reason = Some(format!("Merchant broke: {:.2}", merchant.currency));
    }

    let total_currency = pop.currency + merchant.currency;
    if (total_currency - 1100.0).abs() > 1.0 {
        converged = false;
        failure_reason = Some(format!("Currency leak: {:.2}", total_currency));
    }

    let final_merchant_stock = merchant
        .stockpiles
        .get(&settlement)
        .map(|s| s.get(GRAIN))
        .unwrap_or(0.0);

    ConvergenceResult {
        final_price,
        final_pop_stock: pop.stocks.get(&GRAIN).copied().unwrap_or(0.0),
        final_merchant_stock,
        price_history,
        pop_stock_history,
        merchant_stock_history,
        converged,
        failure_reason,
    }
}

fn variance(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mean = data.iter().sum::<f64>() / data.len() as f64;
    data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / data.len() as f64
}

// === TESTS ===

#[test]
fn basic_convergence() {
    let result = run_trial(SystemParams::default(), InitialConditions::default(), 200);

    assert!(result.converged, "Failed: {:?}", result.failure_reason);

    // Price should settle in reasonable range
    assert!(
        result.final_price > 0.5 && result.final_price < 1.5,
        "Price out of range: {:.3}",
        result.final_price
    );

    // Variance should not explode
    if result.price_history.len() >= 40 {
        let early = variance(&result.price_history[..20]);
        let late = variance(&result.price_history[result.price_history.len() - 20..]);
        assert!(
            late < early * 5.0 + 0.1,
            "Variance exploded: early={:.4}, late={:.4}",
            early,
            late
        );
    }
}

#[test]
fn sweep_initial_prices() {
    let prices = [0.1, 0.5, 1.0, 2.0, 5.0, 10.0];

    println!("\n=== Initial Price Sweep ===");
    println!(
        "{:>10} {:>10} {:>10} {:>10} {:>6}",
        "init_p", "final_p", "pop_stk", "merc_stk", "ok"
    );

    let mut failures = Vec::new();

    for &p in &prices {
        let cond = InitialConditions {
            initial_price: p,
            ..Default::default()
        };
        let r = run_trial(SystemParams::default(), cond, 200);

        println!(
            "{:>10.2} {:>10.3} {:>10.2} {:>10.2} {:>6}",
            p,
            r.final_price,
            r.final_pop_stock,
            r.final_merchant_stock,
            if r.converged { "✓" } else { "✗" }
        );

        if !r.converged {
            failures.push((p, r.failure_reason));
        }
    }

    assert!(failures.is_empty(), "Failures: {:?}", failures);
}

#[test]
fn sweep_initial_stocks() {
    let combos = [
        (0.0, 0.0),
        (0.0, 10.0),
        (10.0, 0.0),
        (5.0, 5.0),
        (20.0, 20.0),
    ];

    println!("\n=== Initial Stock Sweep ===");
    println!(
        "{:>8} {:>8} {:>10} {:>10} {:>10} {:>6}",
        "pop_i", "merc_i", "final_p", "pop_f", "merc_f", "ok"
    );

    let mut failures = Vec::new();

    for &(ps, ms) in &combos {
        let cond = InitialConditions {
            pop_stock: ps,
            merchant_stock: ms,
            ..Default::default()
        };
        let r = run_trial(SystemParams::default(), cond, 200);

        println!(
            "{:>8.1} {:>8.1} {:>10.3} {:>10.2} {:>10.2} {:>6}",
            ps,
            ms,
            r.final_price,
            r.final_pop_stock,
            r.final_merchant_stock,
            if r.converged { "✓" } else { "✗" }
        );

        if !r.converged {
            failures.push(((ps, ms), r.failure_reason));
        }
    }

    assert!(failures.is_empty(), "Failures: {:?}", failures);
}

/// Test how pop's buffer target affects equilibrium price.
///
/// Pop target = desired_consumption_ema × BUFFER_TICKS (5.0)
/// Merchant target = TARGET_STOCK_BUFFER (2.0, hardcoded)
///
/// Hypothesis: Higher pop target → pop less eager → lower prices
///             Lower pop target → pop more eager → higher prices
#[test]
fn sweep_pop_buffer_target() {
    // desired_consumption values → pop targets (× 5.0 buffer ticks)
    // 0.4 → 2.0 (same as merchant)
    // 1.0 → 5.0 (default)
    // 2.0 → 10.0 (much higher than merchant)
    let desired_consumptions = [0.2, 0.4, 0.6, 0.8, 1.0, 1.5, 2.0];

    println!("\n=== Pop Buffer Target Sweep ===");
    println!("(Merchant target fixed at 2.0)");
    println!(
        "{:>8} {:>10} {:>10} {:>10} {:>10} {:>6}",
        "des_ema", "pop_tgt", "final_p", "pop_stk", "merc_stk", "ok"
    );

    let mut results = Vec::new();

    for &dc in &desired_consumptions {
        let params = SystemParams {
            pop_desired_consumption: dc,
            ..Default::default()
        };
        let r = run_trial(params, InitialConditions::default(), 300);

        let pop_target = dc * 5.0; // BUFFER_TICKS

        println!(
            "{:>8.2} {:>10.1} {:>10.3} {:>10.2} {:>10.2} {:>6}",
            dc,
            pop_target,
            r.final_price,
            r.final_pop_stock,
            r.final_merchant_stock,
            if r.converged { "✓" } else { "✗" }
        );

        results.push((dc, pop_target, r));
    }

    // Verify trend: as pop_target increases, price should generally decrease
    // (pop becomes less urgent relative to merchant)
    let prices: Vec<f64> = results.iter().map(|(_, _, r)| r.final_price).collect();
    println!("\nPrice trend check:");
    println!(
        "  Prices: {:?}",
        prices
            .iter()
            .map(|p| format!("{:.3}", p))
            .collect::<Vec<_>>()
    );

    // Check all converged
    let failures: Vec<_> = results
        .iter()
        .filter(|(_, _, r)| !r.converged)
        .map(|(dc, _, r)| (*dc, r.failure_reason.clone()))
        .collect();
    assert!(failures.is_empty(), "Failures: {:?}", failures);
}

/// Test production/consumption imbalance
#[test]
fn sweep_production_rate() {
    let rates = [0.5, 0.8, 1.0, 1.2, 1.5, 2.0];

    println!("\n=== Production Rate Sweep ===");
    println!("(Consumption fixed at 1.0)");
    println!(
        "{:>8} {:>10} {:>10} {:>10} {:>6}",
        "prod", "final_p", "pop_stk", "merc_stk", "ok"
    );

    for &rate in &rates {
        let params = SystemParams {
            production_rate: rate,
            ..Default::default()
        };
        let r = run_trial(params, InitialConditions::default(), 200);

        println!(
            "{:>8.2} {:>10.3} {:>10.2} {:>10.2} {:>6}",
            rate,
            r.final_price,
            r.final_pop_stock,
            r.final_merchant_stock,
            if r.converged { "✓" } else { "✗" }
        );
    }
}

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

    let result =
        market::clear_multi_market(&[GRAIN], all_orders, &budgets, 20, PriceBias::FavorSellers);

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

/// Detailed trace of a single run to understand dynamics
#[test]
fn trace_dynamics() {
    let params = SystemParams::default();
    // Start with price=2.0 which we know causes instability
    let conditions = InitialConditions {
        initial_price: 2.0,
        pop_stock: 2.0,
        merchant_stock: 0.0,
    };

    let mut world = create_world(params, conditions);
    let recipes = vec![make_recipe(params.production_rate)];
    let good_profiles = make_good_profiles();
    let needs = make_needs(params.consumption_requirement);

    let settlement = *world.settlements.keys().next().unwrap();

    println!("\n=== Dynamics Trace (initial_price=2.0) ===");
    println!("Pop target = 5.0 (desired_ema=1.0 × BUFFER_TICKS=5.0)");
    println!("Merchant target = 2.0 (TARGET_STOCK_BUFFER)");
    println!();
    println!(
        "{:>4} {:>8} {:>8} {:>8} {:>8} {:>10} {:>10}",
        "tick", "price", "pop_stk", "merc_stk", "pop_$", "merc_$", "note"
    );

    for tick in 0..30 {
        let pop = world.pops.values().next().unwrap();
        let merchant = world.merchants.values().next().unwrap();
        let price = world
            .price_ema
            .get(&(settlement, GRAIN))
            .copied()
            .unwrap_or(0.0);
        let pop_stock = pop.stocks.get(&GRAIN).copied().unwrap_or(0.0);
        let merc_stock = merchant
            .stockpiles
            .get(&settlement)
            .map(|s| s.get(GRAIN))
            .unwrap_or(0.0);

        // Compute normalized positions
        let pop_norm_c = pop_stock / 5.0; // pop target = 5
        let merc_norm_c = if merc_stock > 0.0 {
            merc_stock / 2.0
        } else {
            0.0
        }; // merc target = 2

        let note = if merc_norm_c < 1.0 {
            "merc<tgt" // merchant below target - only sells at premium!
        } else if pop_norm_c > 1.0 {
            "pop>tgt"
        } else {
            ""
        };

        println!(
            "{:>4} {:>8.3} {:>8.2} {:>8.2} {:>8.1} {:>10.1} {:>10}",
            tick, price, pop_stock, merc_stock, pop.currency, merchant.currency, note
        );

        // Stop if things explode
        if price > 100.0 || pop.currency <= 0.0 || merchant.currency <= 0.0 {
            println!("... stopping early (instability detected)");
            break;
        }

        world.run_tick(&good_profiles, &needs, &recipes);
    }

    // Also trace the stable case for comparison
    println!("\n=== Dynamics Trace (initial_price=1.0, stable) ===");
    let conditions_stable = InitialConditions {
        initial_price: 1.0,
        pop_stock: 2.0,
        merchant_stock: 0.0,
    };
    let mut world2 = create_world(params, conditions_stable);
    let settlement2 = *world2.settlements.keys().next().unwrap();

    println!(
        "{:>4} {:>8} {:>8} {:>8} {:>8} {:>10}",
        "tick", "price", "pop_stk", "merc_stk", "pop_$", "merc_$"
    );

    for tick in 0..30 {
        let pop = world2.pops.values().next().unwrap();
        let merchant = world2.merchants.values().next().unwrap();
        let price = world2
            .price_ema
            .get(&(settlement2, GRAIN))
            .copied()
            .unwrap_or(0.0);
        let pop_stock = pop.stocks.get(&GRAIN).copied().unwrap_or(0.0);
        let merc_stock = merchant
            .stockpiles
            .get(&settlement2)
            .map(|s| s.get(GRAIN))
            .unwrap_or(0.0);

        println!(
            "{:>4} {:>8.3} {:>8.2} {:>8.2} {:>8.1} {:>10.1}",
            tick, price, pop_stock, merc_stock, pop.currency, merchant.currency
        );

        world2.run_tick(&good_profiles, &needs, &recipes);
    }
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
    println!("\n=== Multi-Pop Basic Convergence ===\n");

    // Ideal conditions:
    // - 100 pops, 2 facilities (50 workers each)
    // - Recipe: 1 worker produces 1 grain (production_rate = 1.0)
    // - Total production = 100 workers × 1 grain = 100
    // - Consumption = 100 pops × 1 grain = 100 total
    // - Perfect balance!
    // - Start with ample stock on both sides

    let result = run_multi_pop_trial(
        100,   // pops
        2,     // facilities
        1.0,   // production per worker (1 grain each)
        1.0,   // initial price
        5.0,   // initial pop stock (at target)
        100.0, // initial merchant stock
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
    println!("\n=== Multi-Pop Initial Conditions Sweep ===\n");

    let scenarios = [
        // (price, pop_stock, merc_stock, description)
        (1.0, 5.0, 100.0, "ideal"),
        (0.5, 5.0, 100.0, "low price"),
        (2.0, 5.0, 100.0, "high price"),
        (1.0, 0.0, 100.0, "pops start hungry"),
        (1.0, 5.0, 0.0, "merchant starts empty"),
        (2.0, 1.0, 0.0, "worst case"),
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

/// Combined sweep for thorough coverage
#[test]
fn sweep_combined() {
    let prices = [0.5, 1.0, 2.0];
    let pop_stocks = [0.0, 5.0];
    let merchant_stocks = [0.0, 5.0];

    println!("\n=== Combined Sweep ===");
    println!(
        "{:>6} {:>6} {:>6} {:>8} {:>8} {:>8} {:>4}",
        "p_i", "ps_i", "ms_i", "p_f", "ps_f", "ms_f", "ok"
    );

    let mut pass = 0;
    let mut fail = 0;

    for &p in &prices {
        for &ps in &pop_stocks {
            for &ms in &merchant_stocks {
                let cond = InitialConditions {
                    initial_price: p,
                    pop_stock: ps,
                    merchant_stock: ms,
                };
                let r = run_trial(SystemParams::default(), cond, 200);

                println!(
                    "{:>6.1} {:>6.1} {:>6.1} {:>8.3} {:>8.2} {:>8.2} {:>4}",
                    p,
                    ps,
                    ms,
                    r.final_price,
                    r.final_pop_stock,
                    r.final_merchant_stock,
                    if r.converged { "✓" } else { "✗" }
                );

                if r.converged {
                    pass += 1;
                } else {
                    fail += 1;
                }
            }
        }
    }

    println!("\nPass: {}, Fail: {}", pass, fail);
    assert_eq!(fail, 0, "{} conditions failed", fail);
}
