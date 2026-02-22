#![cfg(feature = "instrument")]

/*
Tests

Standard conditions:
100 pop
- 3 ticks of food buffer, target is 5
2 facilities
- 50 worker slots each
- no marginal extras
1.05 food per slot
subsistence
- k     = 2
- q_max = 1.0
world market:
- grain price = 1.0

freeze mortality and growth for first 10 ticks

expect to converge to:
~100-110 pops
~100% of worker slots filled
food price ~1.0 (or should it be less?)

Above should hold for a variety of starting conditions
- starting pop number: 1-300
- starting buffer 20%-200%

When varying k, expect pop -> ~worker slots + 1.4*k

When varying q_max > 1, expect pop less than worker slots + 2*k
When q_max < 1, expect collapse (wages not tied to viable subsistence floor)
*/

#[allow(dead_code)]
mod common;
use common::*;

use polars::prelude::*;

use sim_core::{
    AnchoredGoodConfig, ExternalMarketConfig, SettlementFriction, SubsistenceReservationConfig,
    World,
    production::{FacilityType, RecipeId},
};

// === SCENARIO & RESULT ===
const SCENARIO_SEED: u64 = 42;

fn scenario_seed() -> u64 {
    std::env::var("SINGLE_GOOD_SEED")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(SCENARIO_SEED)
}

#[derive(Clone, Copy)]
struct Scenario {
    num_pops: usize,
    num_facilities: usize,
    slots_per_facility: usize,
    production_rate: f64,
    initial_pop_stock: f64,
    initial_merchant_stock: f64,
    initial_price: f64,
    subsistence_k: usize,
    subsistence_q_max: f64,
    risk_premium: f64,
    world_grain_price: f64,
    transport_bps: f64,
    mortality_grace_ticks: u64,
    ticks: usize,
    tail_window: usize,
}

impl Scenario {
    /// Standard conditions from the spec.
    fn standard() -> Self {
        Self {
            num_pops: 100,
            num_facilities: 2,
            slots_per_facility: 50,
            production_rate: 1.05,
            initial_pop_stock: 3.0,
            initial_merchant_stock: 200.0,
            initial_price: 1.0,
            subsistence_k: 2,
            subsistence_q_max: 1.0,
            risk_premium: 0.10,
            world_grain_price: 1.0,
            transport_bps: 9000.0,
            mortality_grace_ticks: 10,
            ticks: 600,
            tail_window: 200,
        }
    }

    fn total_capacity(&self) -> usize {
        self.num_facilities * self.slots_per_facility
    }
}

struct SimResult {
    price: TailStats,
    pop: TailStats,
    emp_rate: TailStats,
    food_sat: TailStats,
    total_deaths: usize,
    total_grows: usize,
    final_pop_count: usize,
    extinction: bool,
}

// === WORLD CREATION ===

fn create_world(s: &Scenario) -> World {
    let mut world = World::new();
    world.mortality_grace_ticks = s.mortality_grace_ticks;

    let settlement = world.add_settlement("TestTown", (0.0, 0.0));

    // Merchant
    let merchant = world.add_merchant();
    {
        let m = world.get_merchant_mut(merchant).unwrap();
        m.currency = 10_000.0;
        if s.initial_merchant_stock > 0.0 {
            m.stockpile_at(settlement)
                .add(GRAIN, s.initial_merchant_stock);
        }
    }

    // Facilities
    let mut facility_ids = Vec::new();
    for _ in 0..s.num_facilities {
        let farm = world
            .add_facility(FacilityType::Farm, settlement, merchant)
            .unwrap();
        let f = world.facility_mut(farm).unwrap();
        f.capacity = s.slots_per_facility as u32;
        f.recipe_priorities = vec![RecipeId::new(1)];
        facility_ids.push(farm);
    }

    // Pops — start unemployed so the labor market clears naturally.
    for _ in 0..s.num_pops {
        let pop = world.add_pop(settlement).unwrap();
        let p = world.pop_mut(pop).unwrap();
        p.currency = 100.0;
        p.skills.insert(LABORER);
        p.min_wage = 0.0;
        p.income_ema = 1.0;
        p.stocks.insert(GRAIN, s.initial_pop_stock);
        p.desired_consumption_ema.insert(GRAIN, 1.0);
    }

    // Labor / price EMAs
    let ss = world.settlements.get_mut(&settlement).unwrap();
    ss.wage_ema.insert(LABORER, 1.0);
    ss.price_ema.insert(GRAIN, s.initial_price);

    // External market (gentle anchor around world_grain_price)
    let mut external = ExternalMarketConfig::default();
    external.anchors.insert(
        GRAIN,
        AnchoredGoodConfig {
            world_price: s.world_grain_price,
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
            transport_bps: s.transport_bps,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );
    world.set_external_market(external);

    // Subsistence
    world.set_subsistence_reservation(SubsistenceReservationConfig::new(
        GRAIN,
        s.subsistence_q_max,
        s.subsistence_k,
        10.0,
        s.risk_premium,
    ));

    world
}

// === RUN + METRIC EXTRACTION ===

fn run_scenario_named(s: &Scenario, name: &str) -> SimResult {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use sim_core::instrument::ScopedRecorder;

    let mut rec = ScopedRecorder::new("data/single_good", name);

    let mut world = create_world(s);
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    let seed = scenario_seed() ^ hasher.finish();
    world.set_random_seed(seed);
    let recipes = vec![make_grain_recipe(s.production_rate)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);

    for _ in 0..s.ticks {
        world.run_tick(&good_profiles, &needs, &recipes);
        if world.settlements.values().all(|s| s.pops.is_empty()) {
            break;
        }
    }

    let final_pop_count: usize = world.settlements.values().map(|s| s.pops.len()).sum();
    let extinction = final_pop_count == 0;
    let dfs = rec.get();

    // -- Price --
    let price_stats = extract_price_stats(&dfs, s.tail_window);

    // -- Pop count & mortality --
    let (pop_stats, total_deaths, total_grows) = extract_pop_stats(&dfs, s.tail_window);

    // -- Food satisfaction --
    let food_sat_stats = extract_food_sat_stats(&dfs, s.tail_window);

    // -- Employment rate --
    let emp_rate_stats = extract_emp_rate_stats(&dfs, s.tail_window);

    SimResult {
        price: price_stats,
        pop: pop_stats,
        emp_rate: emp_rate_stats,
        food_sat: food_sat_stats,
        total_deaths,
        total_grows,
        final_pop_count,
        extinction,
    }
}

// === DATAFRAME HELPERS ===

fn extract_price_stats(
    dfs: &std::collections::HashMap<String, DataFrame>,
    tail_window: usize,
) -> TailStats {
    if let Some(fill) = dfs.get("fill") {
        let prices_by_tick = fill
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("price").mean().alias("price")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let series = col_f64(&prices_by_tick, "price");
        compute_tail_stats(&series, tail_window)
    } else {
        compute_tail_stats(&[], tail_window)
    }
}

fn extract_pop_stats(
    dfs: &std::collections::HashMap<String, DataFrame>,
    tail_window: usize,
) -> (TailStats, usize, usize) {
    if let Some(mortality) = dfs.get("mortality") {
        let pop_by_tick = mortality
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("pop_id").n_unique().alias("pop_count")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let series = col_f64(&pop_by_tick, "pop_count");
        let stats = compute_tail_stats(&series, tail_window);

        let totals = mortality
            .clone()
            .lazy()
            .select([
                col("outcome")
                    .eq(lit("dies"))
                    .cast(DataType::Int32)
                    .sum()
                    .alias("deaths"),
                col("outcome")
                    .eq(lit("grows"))
                    .cast(DataType::Int32)
                    .sum()
                    .alias("grows"),
            ])
            .collect()
            .unwrap();
        let deaths = totals
            .column("deaths")
            .unwrap()
            .i32()
            .unwrap()
            .get(0)
            .unwrap_or(0) as usize;
        let grows = totals
            .column("grows")
            .unwrap()
            .i32()
            .unwrap()
            .get(0)
            .unwrap_or(0) as usize;

        (stats, deaths, grows)
    } else if let Some(consumption) = dfs.get("consumption") {
        let pop_by_tick = consumption
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("pop_id").n_unique().alias("pop_count")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let series = col_f64(&pop_by_tick, "pop_count");
        let stats = compute_tail_stats(&series, tail_window);
        (stats, 0, 0)
    } else {
        (compute_tail_stats(&[], tail_window), 0, 0)
    }
}

fn extract_food_sat_stats(
    dfs: &std::collections::HashMap<String, DataFrame>,
    tail_window: usize,
) -> TailStats {
    if let Some(mortality) = dfs.get("mortality") {
        let sat_by_tick = mortality
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("food_satisfaction").mean().alias("food_sat")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let series = col_f64(&sat_by_tick, "food_sat");
        compute_tail_stats(&series, tail_window)
    } else if let Some(consumption) = dfs.get("consumption") {
        let sat_by_tick = consumption
            .clone()
            .lazy()
            .with_column(
                when(col("desired").gt(lit(0.0)))
                    .then(
                        when((col("actual") / col("desired")).lt(lit(0.0)))
                            .then(lit(0.0))
                            .otherwise(
                                when((col("actual") / col("desired")).gt(lit(1.0)))
                                    .then(lit(1.0))
                                    .otherwise(col("actual") / col("desired")),
                            ),
                    )
                    .otherwise(lit(1.0))
                    .alias("food_sat"),
            )
            .group_by([col("tick")])
            .agg([col("food_sat").mean().alias("food_sat")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let series = col_f64(&sat_by_tick, "food_sat");
        compute_tail_stats(&series, tail_window)
    } else {
        compute_tail_stats(&[], tail_window)
    }
}

fn extract_emp_rate_stats(
    dfs: &std::collections::HashMap<String, DataFrame>,
    tail_window: usize,
) -> TailStats {
    if let Some(assignment) = dfs.get("assignment") {
        let pop_source = if let Some(mortality) = dfs.get("mortality") {
            mortality
        } else if let Some(consumption) = dfs.get("consumption") {
            consumption
        } else {
            return compute_tail_stats(&[], tail_window);
        };
        let pop_by_tick = pop_source
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("pop_id").n_unique().alias("pop_count")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let emp_by_tick = assignment
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("pop_id").n_unique().alias("employed")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let merged = pop_by_tick
            .lazy()
            .join(
                emp_by_tick.lazy(),
                [col("tick")],
                [col("tick")],
                JoinArgs::new(JoinType::Left),
            )
            .with_column(col("employed").fill_null(lit(0u32)))
            .with_column(
                (col("employed").cast(DataType::Float64)
                    / col("pop_count").cast(DataType::Float64))
                .alias("emp_rate"),
            )
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let series = col_f64(&merged, "emp_rate");
        compute_tail_stats(&series, tail_window)
    } else {
        compute_tail_stats(&[], tail_window)
    }
}

fn print_result(label: &str, r: &SimResult) {
    println!("\n--- {} ---", label);
    println!("  Price:    {}", r.price);
    println!("  Pop:      {}", r.pop);
    println!("  EmpRate:  {}", r.emp_rate);
    println!("  FoodSat:  {}", r.food_sat);
    println!(
        "  Deaths: {}, Grows: {}, Final pop: {}",
        r.total_deaths, r.total_grows, r.final_pop_count
    );
}

// === EQUILIBRIUM PREDICTION ===

/// Predict steady-state population from production + subsistence balance.
fn predict_equilibrium_pop(
    total_capacity: usize,
    production_rate: f64,
    subsistence_k: usize,
    subsistence_q_max: f64,
) -> usize {
    use sim_core::labor::subsistence::subsistence_output_per_worker;

    // Search for pop N where total_output >= N * 1.0 (consumption requirement)
    // and total_output(N+1) < (N+1)
    let max_search = total_capacity + 4 * subsistence_k + 50;
    let mut best_n = total_capacity;

    for n in 1..=max_search {
        let employed = n.min(total_capacity);
        let unemployed = n.saturating_sub(employed);
        let formal = employed as f64 * production_rate;
        let subsistence: f64 = (1..=unemployed)
            .map(|rank| subsistence_output_per_worker(rank, subsistence_q_max, subsistence_k))
            .sum();
        let surplus = formal + subsistence - n as f64;
        if surplus >= 0.0 {
            best_n = n;
        } else {
            break;
        }
    }

    best_n
}

// === TESTS ===

#[test]
fn standard_convergence() {
    let s = Scenario {
        subsistence_k: 50,
        world_grain_price: 10.,
        ..Scenario::standard()
    };

    let predicted = predict_equilibrium_pop(
        s.total_capacity(),
        s.production_rate,
        s.subsistence_k,
        s.subsistence_q_max,
    );
    println!("\nPredicted equilibrium pop: {}", predicted);

    let r = run_scenario_named(&s, "standard_convergence");
    print_result("standard", &r);

    assert!(!r.extinction, "population went extinct");

    // With world_grain_price=10 and transport_bps=9000, the export floor is
    // 10 * (1 - 0.95) = 0.50, which anchors the price EMA.
    // Pop = worker_slots + ~1.4*k ≈ 100 + 70 = 170 with k=50.
    assert!(
        r.pop.mean > (predicted as f64 * 0.85) && r.pop.mean < (predicted as f64 * 1.15),
        "tail pop mean {:.1} outside ±15% of predicted {}",
        r.pop.mean,
        predicted,
    );
    assert!(r.pop.cv < 0.15, "tail pop CV too high: {:.4}", r.pop.cv);

    // All worker slots should be filled (employed = emp_rate * pop ≈ 100)
    let capacity = s.total_capacity() as f64;
    let employed = r.emp_rate.mean * r.pop.mean;
    assert!(
        employed > capacity * 0.90,
        "employed {:.0} below 90% of capacity {:.0}",
        employed,
        capacity,
    );

    // Price anchored near export floor (0.50)
    assert!(
        r.price.mean > 0.40 && r.price.mean < 0.60,
        "tail price {:.4} outside expected 0.40-0.60",
        r.price.mean,
    );

    // Food satisfaction healthy
    assert!(
        r.food_sat.mean > 0.90,
        "tail food satisfaction {:.4} too low",
        r.food_sat.mean,
    );
}

/// Investigation: what happens with world_price=1.0 (export floor=0.05)?
#[test]
#[ignore]
fn investigate_low_export_floor() {
    // Export floor = 1.0 * (1 - 0.95) = 0.05
    let s = Scenario {
        subsistence_k: 50,
        world_grain_price: 1.0,
        ..Scenario::standard()
    };
    let r = run_scenario_named(&s, "low_export_floor");
    print_result("export_floor=0.05", &r);

    // Export floor = 20.0 * (1 - 0.95) = 1.0
    let s2 = Scenario {
        subsistence_k: 50,
        world_grain_price: 20.0,
        ..Scenario::standard()
    };
    let r2 = run_scenario_named(&s2, "high_export_floor");
    print_result("export_floor=1.0", &r2);
}

/// Investigation: does 10% production margin fix the high export floor instability?
#[test]
#[ignore]
fn investigate_production_margin() {
    // 5% margin at export floor=1.0 (the unstable case)
    let s5 = Scenario {
        subsistence_k: 50,
        world_grain_price: 20.0,
        production_rate: 1.05,
        ..Scenario::standard()
    };
    let r5 = run_scenario_named(&s5, "margin_5pct");
    print_result("5% margin, floor=1.0", &r5);

    // 10% margin at export floor=1.0
    let s10 = Scenario {
        subsistence_k: 50,
        world_grain_price: 20.0,
        production_rate: 1.10,
        ..Scenario::standard()
    };
    let r10 = run_scenario_named(&s10, "margin_10pct");
    print_result("10% margin, floor=1.0", &r10);
}

/// The same equilibrium should be reached from different starting conditions.
#[test]
#[ignore = "stochastic sweep characterization; run explicitly with --ignored"]
fn varying_starting_conditions() {
    println!("\n=== Varying Starting Conditions ===\n");

    // (num_pops, initial_pop_stock, label)
    let conditions: &[(usize, f64, &str)] = &[
        (50, 3.0, "50_pop_normal_buffer"),
        (100, 1.0, "100_pop_low_buffer"),
        (100, 10.0, "100_pop_high_buffer"),
        (150, 3.0, "150_pop_normal_buffer"),
        (200, 3.0, "200_pop_normal_buffer"),
    ];

    // Use a moderate k with a meaningful export floor.
    // k=10 → predicted ≈ 114, reachable from all starting sizes.
    let base = Scenario {
        subsistence_k: 10,
        world_grain_price: 10.0,
        ..Scenario::standard()
    };
    let predicted = predict_equilibrium_pop(
        base.total_capacity(),
        base.production_rate,
        base.subsistence_k,
        base.subsistence_q_max,
    );

    let mut tail_prices = Vec::new();
    let mut tail_pops = Vec::new();

    for &(num_pops, pop_stock, label) in conditions {
        let s = Scenario {
            num_pops,
            initial_pop_stock: pop_stock,
            // Scale merchant stock with pop count
            initial_merchant_stock: num_pops as f64 * 2.0,
            // Longer run for populations that need to grow/shrink a lot
            ticks: if num_pops <= 50 || num_pops >= 200 {
                1000
            } else {
                600
            },
            ..base
        };

        let r = run_scenario_named(&s, label);
        print_result(label, &r);

        assert!(!r.extinction, "{}: went extinct", label);

        // All should converge near the same equilibrium
        assert!(
            r.pop.mean > (predicted as f64 * 0.7) && r.pop.mean < (predicted as f64 * 1.5),
            "{}: tail pop {:.0} far from predicted {}",
            label,
            r.pop.mean,
            predicted,
        );

        assert!(
            r.food_sat.mean > 0.85,
            "{}: food satisfaction {:.4} too low",
            label,
            r.food_sat.mean,
        );

        tail_prices.push(r.price.mean);
        tail_pops.push(r.pop.mean);
    }

    // Cross-scenario: prices should converge to similar attractor
    let price_min = tail_prices.iter().cloned().fold(f64::INFINITY, f64::min);
    let price_max = tail_prices
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let price_band = price_max - price_min;
    println!(
        "\nPrice band: {:.4} (prices: {:?})",
        price_band, tail_prices
    );
    assert!(
        price_band < 0.50,
        "prices diverged across starting conditions: band={:.4}, prices={:?}",
        price_band,
        tail_prices,
    );

    // Populations should converge to similar range
    let pop_min = tail_pops.iter().cloned().fold(f64::INFINITY, f64::min);
    let pop_max = tail_pops.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let pop_band = pop_max - pop_min;
    println!("Pop band: {:.1} (pops: {:?})", pop_band, tail_pops);
    assert!(
        pop_band < 40.0,
        "populations diverged: band={:.1}, pops={:?}",
        pop_band,
        tail_pops,
    );
}

/// When varying k, equilibrium pop should track ~worker_slots + 1.4*k.
#[test]
#[ignore = "stochastic sweep characterization; run explicitly with --ignored"]
fn varying_k_shifts_equilibrium() {
    println!("\n=== Varying k ===\n");

    let ks: &[usize] = &[2, 5, 10, 20];

    for &k in ks {
        let s = Scenario {
            subsistence_k: k,
            world_grain_price: 10.0,
            ticks: 800,
            ..Scenario::standard()
        };

        let predicted = predict_equilibrium_pop(
            s.total_capacity(),
            s.production_rate,
            s.subsistence_k,
            s.subsistence_q_max,
        );
        let approx = s.total_capacity() as f64 + 1.4 * k as f64;

        let r = run_scenario_named(&s, &format!("varying_k_{}", k));
        print_result(&format!("k={}", k), &r);

        assert!(!r.extinction, "k={}: went extinct", k);

        println!(
            "  k={}: predicted={}, approx={:.0}, actual_mean={:.1}",
            k, predicted, approx, r.pop.mean
        );

        // Pop should be near the analytical prediction
        assert!(
            r.pop.mean > (predicted as f64 * 0.8) && r.pop.mean < (predicted as f64 * 1.3),
            "k={}: tail pop {:.0} far from predicted {}",
            k,
            r.pop.mean,
            predicted,
        );

        // Food satisfaction should remain healthy
        assert!(
            r.food_sat.mean > 0.85,
            "k={}: food sat {:.4} too low",
            k,
            r.food_sat.mean,
        );
    }
}

/// With q_max > 1, equilibrium pop should be less than worker_slots + 2*k.
#[test]
fn q_max_above_one_bounded() {
    println!("\n=== q_max > 1 ===\n");

    let s = Scenario {
        subsistence_q_max: 1.5,
        ticks: 800,
        ..Scenario::standard()
    };

    let upper_bound = s.total_capacity() + 2 * s.subsistence_k;

    let r = run_scenario_named(&s, "q_max_above_one");
    print_result("q_max=1.5", &r);

    assert!(!r.extinction, "q_max=1.5: went extinct");

    assert!(
        r.pop.mean < upper_bound as f64,
        "q_max=1.5: tail pop {:.0} should be < worker_slots + 2*k = {}",
        r.pop.mean,
        upper_bound,
    );

    // Should still be healthy
    assert!(
        r.food_sat.mean > 0.85,
        "q_max=1.5: food sat {:.4} too low",
        r.food_sat.mean,
    );
}

/// With q_max < 1 and a meaningful price floor, subsistence can't sustain
/// food_sat=1.0 → reservation wages are low → wages grind down → stress.
/// At the standard low export floor (price≈0.05) pops survive trivially,
/// so we test with a higher export floor where the margin matters.
#[test]
fn q_max_below_one_collapses() {
    println!("\n=== q_max < 1 (expect collapse) ===\n");

    let s = Scenario {
        subsistence_q_max: 0.5,
        world_grain_price: 20.0, // export floor ≈ 1.0
        subsistence_k: 50,
        ticks: 600,
        ..Scenario::standard()
    };

    let r = run_scenario_named(&s, "q_max_below_one");
    print_result("q_max=0.5", &r);

    // With q_max=0.5 at a meaningful price floor, subsistence can't sustain
    // food_sat=1.0 for unemployed. The reservation wage is low, so when
    // employment oscillates, pops die faster than they can recover.
    let started_with = s.num_pops;
    let lost_fraction = 1.0 - (r.final_pop_count as f64 / started_with as f64);
    println!(
        "  Lost {:.0}% of starting pop ({} -> {})",
        lost_fraction * 100.0,
        started_with,
        r.final_pop_count,
    );

    assert!(
        r.extinction || lost_fraction > 0.30,
        "q_max=0.5: expected collapse but only lost {:.0}% of pop",
        lost_fraction * 100.0,
    );
}
