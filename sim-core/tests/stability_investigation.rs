#[allow(dead_code)]
mod common;
use common::*;

use polars::prelude::*;

// Re-import common::mean to shadow polars::prelude::mean
use common::mean;
use sim_core::instrument::ScopedRecorder;
use sim_core::production::{FacilityType, RecipeId};
use sim_core::{AnchoredGoodConfig, ExternalMarketConfig, SettlementFriction, World};

#[derive(Debug, Clone, Copy)]
struct Scenario {
    name: &'static str,
    depth_per_pop: f64,
    transport_bps: f64,
}


fn create_world(
    num_pops: usize,
    num_facilities: usize,
    initial_price: f64,
    initial_pop_stock: f64,
    initial_merchant_stock: f64,
) -> (World, sim_core::SettlementId) {
    let mut world = World::new();
    let settlement = world.add_settlement("InvestigateTown", (0.0, 0.0));

    let merchant = world.add_merchant();
    {
        let m = world.get_merchant_mut(merchant).unwrap();
        m.currency = 10_000.0;
        m.stockpile_at(settlement)
            .add(GRAIN, initial_merchant_stock);
    }

    let workers_per_facility = num_pops.div_ceil(num_facilities);
    let mut facility_ids = Vec::new();
    for _ in 0..num_facilities {
        let farm = world
            .add_facility(FacilityType::Farm, settlement, merchant)
            .unwrap();
        let f = world.get_facility_mut(farm).unwrap();
        f.capacity = workers_per_facility as u32;
        f.recipe_priorities = vec![RecipeId::new(1)];
        facility_ids.push(farm);
    }

    for i in 0..num_pops {
        let pop = world.add_pop(settlement).unwrap();
        let facility = facility_ids[i % num_facilities];
        let p = world.get_pop_mut(pop).unwrap();
        p.currency = 100.0;
        p.skills.insert(LABORER);
        p.min_wage = 0.5;
        p.employed_at = Some(facility);
        p.income_ema = 1.0;
        p.stocks.insert(GRAIN, initial_pop_stock);
        p.desired_consumption_ema.insert(GRAIN, 1.0);
    }

    world.wage_ema.insert(LABORER, 1.0);
    world.price_ema.insert((settlement, GRAIN), initial_price);

    (world, settlement)
}

fn configure_anchor(
    world: &mut World,
    settlement: sim_core::SettlementId,
    depth_per_pop: f64,
    transport_bps: f64,
) {
    let mut external = ExternalMarketConfig::default();
    external.anchors.insert(
        GRAIN,
        AnchoredGoodConfig {
            world_price: 10.0,
            spread_bps: 500.0,
            base_depth: 0.0,
            depth_per_pop,
            tiers: 9,
            tier_step_bps: 300.0,
        },
    );
    external.frictions.insert(
        settlement,
        SettlementFriction {
            enabled: true,
            transport_bps,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );
    world.set_external_market(external);
}


fn col_f64(df: &DataFrame, name: &str) -> Vec<f64> {
    let series = df.column(name).unwrap();
    match series.dtype() {
        polars::datatypes::DataType::Float64 => {
            series.f64().unwrap().into_no_null_iter().collect()
        }
        polars::datatypes::DataType::UInt64 => {
            series.u64().unwrap().into_no_null_iter().map(|v| v as f64).collect()
        }
        polars::datatypes::DataType::UInt32 => {
            series.u32().unwrap().into_no_null_iter().map(|v| v as f64).collect()
        }
        polars::datatypes::DataType::Int32 => {
            series.i32().unwrap().into_no_null_iter().map(|v| v as f64).collect()
        }
        polars::datatypes::DataType::Int64 => {
            series.i64().unwrap().into_no_null_iter().map(|v| v as f64).collect()
        }
        dt => panic!("col_f64: unsupported dtype {dt:?} for column {name}"),
    }
}

#[test]
#[ignore = "investigation workflow; run manually"]
fn investigate_anchor_regimes_with_dataframes() {
    let scenarios = [
        Scenario {
            name: "balanced_candidate",
            depth_per_pop: 0.10,
            transport_bps: 9000.0,
        },
        Scenario {
            name: "low_price_collapse",
            depth_per_pop: 0.20,
            transport_bps: 11000.0,
        },
        Scenario {
            name: "high_price_spike",
            depth_per_pop: 0.20,
            transport_bps: 7000.0,
        },
    ];

    let recipes = vec![make_grain_recipe(1.0)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);

    println!("\n=== Stability Investigation (Instrumented DataFrames) ===");
    println!(
        "{:>18} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "scenario", "tail_p", "tail_emp", "no_trade", "overlap", "fill_rate", "imports", "exports"
    );

    for scenario in scenarios {
        let mut world_and_settlement = create_world(100, 2, 1.0, 5.0, 210.0);
        let (world, settlement) = (&mut world_and_settlement.0, world_and_settlement.1);
        configure_anchor(
            world,
            settlement,
            scenario.depth_per_pop,
            scenario.transport_bps,
        );

        let mut rec = ScopedRecorder::new("data/investigation", scenario.name);
        for _ in 0..220 {
            world.run_tick(&good_profiles, &needs, &recipes);
        }

        let run_dir = rec.run_dir().display().to_string();
        let dfs = rec.get();

        let fill = dfs.get("fill").expect("fill dataframe");
        let order = dfs.get("order").expect("order dataframe");
        let assignment = dfs.get("assignment").expect("assignment dataframe");

        let prices_by_tick = fill
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("price").mean().alias("price")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let price_series = col_f64(&prices_by_tick, "price");
        let tail_price = mean(trailing(&price_series, 40));

        let emp_by_tick = assignment
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("pop_id").count().alias("employed"),
                col("pop_id").n_unique().alias("unique_workers"),
            ])
            .with_column((col("employed").cast(DataType::Float64) / lit(100.0)).alias("emp_rate"))
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let emp_series = col_f64(&emp_by_tick, "emp_rate");
        let tail_emp = mean(trailing(&emp_series, 40));

        let traded_ticks = fill
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("volume")])
            .with_column(
                (col("volume").gt(lit(1e-12)))
                    .cast(DataType::Int32)
                    .alias("had_trade"),
            )
            .select([col("had_trade").sum().alias("traded_ticks")])
            .collect()
            .unwrap();
        let traded = traded_ticks
            .column("traded_ticks")
            .unwrap()
            .i32()
            .unwrap()
            .get(0)
            .unwrap_or(0) as f64;
        let no_trade_share = 1.0 - traded / 220.0;

        let trade_decomp = order
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("quantity")
                    .filter(
                        col("agent_type")
                            .eq(lit("merchant"))
                            .and(col("side").eq(lit("sell"))),
                    )
                    .sum()
                    .alias("offered"),
                col("quantity")
                    .filter(
                        col("agent_type")
                            .eq(lit("pop"))
                            .and(col("side").eq(lit("buy"))),
                    )
                    .sum()
                    .alias("pop_buy"),
            ])
            .join(
                fill.clone()
                    .lazy()
                    .group_by([col("tick")])
                    .agg([col("quantity").sum().alias("filled_total")]),
                [col("tick")],
                [col("tick")],
                JoinArgs::new(JoinType::Left),
            )
            .with_column(col("filled_total").fill_null(lit(0.0)))
            .with_column(
                (col("offered").lt_eq(lit(1e-12)))
                    .cast(DataType::Int32)
                    .alias("offered_zero"),
            )
            .with_column(
                (col("offered")
                    .gt(lit(1e-12))
                    .and(col("filled_total").lt_eq(lit(1e-12))))
                .cast(DataType::Int32)
                .alias("offered_but_no_trade"),
            )
            .with_column(
                (col("pop_buy").lt_eq(lit(1e-12)))
                    .cast(DataType::Int32)
                    .alias("pop_buy_zero"),
            )
            .select([
                col("offered_zero").mean().alias("offered_zero_share"),
                col("offered_but_no_trade")
                    .mean()
                    .alias("offered_but_no_trade_share"),
                col("pop_buy_zero").mean().alias("pop_buy_zero_share"),
                col("pop_buy").mean().alias("avg_pop_buy_qty"),
            ])
            .collect()
            .unwrap();
        let offered_zero_share = trade_decomp
            .column("offered_zero_share")
            .unwrap()
            .f64()
            .unwrap()
            .get(0)
            .unwrap_or(0.0);
        let offered_but_no_trade_share = trade_decomp
            .column("offered_but_no_trade_share")
            .unwrap()
            .f64()
            .unwrap()
            .get(0)
            .unwrap_or(0.0);
        let pop_buy_zero_share = trade_decomp
            .column("pop_buy_zero_share")
            .unwrap()
            .f64()
            .unwrap()
            .get(0)
            .unwrap_or(0.0);
        let avg_pop_buy_qty = trade_decomp
            .column("avg_pop_buy_qty")
            .unwrap()
            .f64()
            .unwrap()
            .get(0)
            .unwrap_or(0.0);

        let overlap = order
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("limit_price")
                    .filter(
                        col("agent_type")
                            .eq(lit("pop"))
                            .and(col("side").eq(lit("buy"))),
                    )
                    .max()
                    .alias("max_bid"),
                col("limit_price")
                    .filter(
                        col("agent_type")
                            .eq(lit("merchant"))
                            .and(col("side").eq(lit("sell"))),
                    )
                    .min()
                    .alias("min_ask"),
            ])
            .with_column((col("max_bid") - col("min_ask")).alias("overlap"))
            .select([col("overlap").mean().alias("avg_overlap")])
            .collect()
            .unwrap();
        let avg_overlap = overlap
            .column("avg_overlap")
            .unwrap()
            .f64()
            .unwrap()
            .get(0)
            .unwrap_or(0.0);

        let fill_rate_df = order
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("quantity")
                .filter(
                    col("agent_type")
                        .eq(lit("merchant"))
                        .and(col("side").eq(lit("sell"))),
                )
                .sum()
                .alias("offered")])
            .join(
                fill.clone()
                    .lazy()
                    .group_by([col("tick")])
                    .agg([col("quantity")
                        .filter(
                            col("agent_type")
                                .eq(lit("merchant"))
                                .and(col("side").eq(lit("sell"))),
                        )
                        .sum()
                        .alias("sold")]),
                [col("tick")],
                [col("tick")],
                JoinArgs::new(JoinType::Left),
            )
            .with_column(col("sold").fill_null(lit(0.0)))
            .with_column(
                when(col("offered").gt(lit(1e-12)))
                    .then(col("sold") / col("offered"))
                    .otherwise(lit(0.0))
                    .alias("fill_rate"),
            )
            .select([col("fill_rate").mean().alias("avg_fill_rate")])
            .collect()
            .unwrap();
        let avg_fill_rate = fill_rate_df
            .column("avg_fill_rate")
            .unwrap()
            .f64()
            .unwrap()
            .get(0)
            .unwrap_or(0.0);

        let (imports, exports) = if let Some(flow) = dfs.get("external_flow") {
            let totals = flow
                .clone()
                .lazy()
                .group_by([col("flow")])
                .agg([col("quantity").sum().alias("qty")])
                .collect()
                .unwrap();
            let mut imports = 0.0;
            let mut exports = 0.0;
            for i in 0..totals.height() {
                let flow_name = totals
                    .column("flow")
                    .unwrap()
                    .str()
                    .unwrap()
                    .get(i)
                    .unwrap_or("");
                let qty = totals
                    .column("qty")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .get(i)
                    .unwrap_or(0.0);
                match flow_name {
                    "import" => imports = qty,
                    "export" => exports = qty,
                    _ => {}
                }
            }
            (imports, exports)
        } else {
            (0.0, 0.0)
        };

        let consumption_fulfillment = if let Some(cons) = dfs.get("consumption") {
            let by_tick = cons
                .clone()
                .lazy()
                .group_by([col("tick")])
                .agg([
                    col("actual").sum().alias("actual"),
                    col("desired").sum().alias("desired"),
                ])
                .with_column(
                    when(col("desired").gt(lit(1e-12)))
                        .then(col("actual") / col("desired"))
                        .otherwise(lit(0.0))
                        .alias("fulfill"),
                )
                .sort(["tick"], Default::default())
                .collect()
                .unwrap();
            let fulfill = col_f64(&by_tick, "fulfill");
            mean(trailing(&fulfill, 40))
        } else {
            0.0
        };

        let (avg_food_sat, min_food_sat, deaths, grows) = if let Some(m) = dfs.get("mortality") {
            let x = m
                .clone()
                .lazy()
                .select([
                    col("food_satisfaction").mean().alias("avg_sat"),
                    col("food_satisfaction").min().alias("min_sat"),
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
            let avg_sat = x
                .column("avg_sat")
                .unwrap()
                .f64()
                .unwrap()
                .get(0)
                .unwrap_or(0.0);
            let min_sat = x
                .column("min_sat")
                .unwrap()
                .f64()
                .unwrap()
                .get(0)
                .unwrap_or(0.0);
            let deaths = x
                .column("deaths")
                .unwrap()
                .i32()
                .unwrap()
                .get(0)
                .unwrap_or(0);
            let grows = x
                .column("grows")
                .unwrap()
                .i32()
                .unwrap()
                .get(0)
                .unwrap_or(0);
            (avg_sat, min_sat, deaths, grows)
        } else {
            (0.0, 0.0, 0, 0)
        };

        let wage_price_ratio = {
            let wages = assignment
                .clone()
                .lazy()
                .group_by([col("tick")])
                .agg([col("wage").mean().alias("avg_wage")]);
            let prices = fill
                .clone()
                .lazy()
                .group_by([col("tick")])
                .agg([col("price").mean().alias("avg_price")]);
            let merged = wages
                .join(
                    prices,
                    [col("tick")],
                    [col("tick")],
                    JoinArgs::new(JoinType::Inner),
                )
                .with_column((col("avg_wage") / col("avg_price")).alias("wage_price_ratio"))
                .collect()
                .unwrap();
            let ratios = col_f64(&merged, "wage_price_ratio");
            mean(trailing(&ratios, 40))
        };

        println!(
            "{:>18} {:>10.4} {:>10.4} {:>10.4} {:>10.4} {:>10.4} {:>10.2} {:>10.2}",
            scenario.name,
            tail_price,
            tail_emp,
            no_trade_share,
            avg_overlap,
            avg_fill_rate,
            imports,
            exports
        );
        println!("  parquet: {}", run_dir);
        println!(
            "  deep: offered_zero={:.4} offered_no_trade={:.4} pop_buy_zero={:.4} pop_buy_qty={:.4} fulfill={:.4} sat_avg={:.4} sat_min={:.4} deaths={} grows={} wage/price={:.4}",
            offered_zero_share,
            offered_but_no_trade_share,
            pop_buy_zero_share,
            avg_pop_buy_qty,
            consumption_fulfillment,
            avg_food_sat,
            min_food_sat,
            deaths,
            grows,
            wage_price_ratio
        );

        // Root-cause guardrails for interpretation:
        // 1) If tail price is extreme but overlap is negative, price symptom likely stems from
        //    persistent book separation and budget/ask mismatch.
        // 2) If fill_rate is low with positive overlap, likely supply withholding / ladder shape issue.
        // 3) If imports+exports near zero in anchor scenarios, anchor is not materially engaged.
    }
}

/// Analytical investigation: why do stress scenarios get trapped at lower populations?
///
/// The worst_case scenario (price=2.0, pop_stock=2.0, merchant_stock=1.0) starts with
/// 100 pops and 2 facilities (capacity 100), production_rate=1.05 — enough to feed 100.
/// But it stabilizes at ~41 pops. This test instruments a worst_case run and queries the
/// dataframes to test specific claims about WHY recovery stalls.
#[test]
#[ignore = "analytical investigation; run manually"]
fn investigate_population_trap() {
    use sim_core::labor::SubsistenceReservationConfig;

    // Set up worst_case scenario matching convergence.rs sweep
    let initial_price = 2.0;
    let initial_pop_stock = 2.0;
    let initial_merchant_stock = 1.0;
    let num_pops = 100;
    let num_facilities = 2;
    let production_rate = 1.05;
    let ticks = 300; // longer to see if recovery ever starts

    let (mut world, settlement) = create_world(
        num_pops,
        num_facilities,
        initial_price,
        initial_pop_stock,
        initial_merchant_stock,
    );

    // Enable same stabilizers as convergence sweep
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
            transport_bps: 9000.0,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );
    world.set_external_market(external);
    world.set_subsistence_reservation(SubsistenceReservationConfig::new(GRAIN, 1.5, 10, 10.0));

    let recipes = vec![make_grain_recipe(production_rate)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);

    let mut rec = ScopedRecorder::new("data/investigation", "population_trap");
    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);
    }
    let dfs = rec.get();

    println!("\n{}", "=".repeat(70));
    println!("=== POPULATION TRAP INVESTIGATION ===");
    println!("=== worst_case: price=2.0, pop_stock=2.0, merchant_stock=1.0 ===");
    println!("{}\n", "=".repeat(70));

    // ── CLAIM 1: Deaths cluster in early ticks, then stop ──
    // If the death spiral is a transient shock, we should see deaths in early ticks
    // and growth in later ticks (if recovery is happening).
    println!("--- CLAIM 1: Death timing ---");
    let mortality = dfs.get("mortality").expect("mortality dataframe");
    let deaths_by_tick = mortality
        .clone()
        .lazy()
        .filter(col("outcome").eq(lit("dies")))
        .group_by([col("tick")])
        .agg([col("pop_id").count().alias("deaths")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let grows_by_tick = mortality
        .clone()
        .lazy()
        .filter(col("outcome").eq(lit("grows")))
        .group_by([col("tick")])
        .agg([col("pop_id").count().alias("grows")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let pop_count_by_tick = mortality
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([col("pop_id").count().alias("pop_count")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    let death_ticks = col_f64(&deaths_by_tick, "tick");
    let death_counts: Vec<f64> = deaths_by_tick
        .column("deaths")
        .unwrap()
        .u32()
        .unwrap()
        .into_no_null_iter()
        .map(|v| v as f64)
        .collect();
    let grow_ticks = col_f64(&grows_by_tick, "tick");

    let total_deaths: f64 = death_counts.iter().sum();
    let last_death_tick = death_ticks.last().copied().unwrap_or(0.0);
    let first_grow_tick = grow_ticks.first().copied().unwrap_or(f64::INFINITY);
    let total_grows: f64 = grows_by_tick
        .column("grows")
        .unwrap()
        .u32()
        .unwrap()
        .into_no_null_iter()
        .map(|v| v as f64)
        .sum();

    println!("  Total deaths: {total_deaths:.0}");
    println!("  Last death tick: {last_death_tick:.0}");
    println!("  Total grows: {total_grows:.0}");
    println!("  First grow tick: {first_grow_tick:.0}");

    // Show death timeline in phases
    let early_deaths: f64 = death_ticks
        .iter()
        .zip(death_counts.iter())
        .filter(|&(&t, _)| t < 20.0)
        .map(|(_, &d)| d)
        .sum();
    let mid_deaths: f64 = death_ticks
        .iter()
        .zip(death_counts.iter())
        .filter(|&(&t, _)| t >= 20.0 && t < 100.0)
        .map(|(_, &d)| d)
        .sum();
    let late_deaths: f64 = death_ticks
        .iter()
        .zip(death_counts.iter())
        .filter(|&(&t, _)| t >= 100.0)
        .map(|(_, &d)| d)
        .sum();
    println!("  Deaths by phase: early(<20)={early_deaths:.0} mid(20-99)={mid_deaths:.0} late(100+)={late_deaths:.0}");

    // ── CLAIM 2: Food satisfaction drops below 0.9 during crisis, preventing growth ──
    println!("\n--- CLAIM 2: Food satisfaction timeline ---");
    let consumption = dfs.get("consumption").expect("consumption dataframe");
    let food_sat_by_tick = mortality
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([
            col("food_satisfaction").mean().alias("avg_food_sat"),
            col("food_satisfaction").min().alias("min_food_sat"),
            col("growth_prob").mean().alias("avg_growth_prob"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let avg_sats = col_f64(&food_sat_by_tick, "avg_food_sat");
    let _min_sats = col_f64(&food_sat_by_tick, "min_food_sat");
    let avg_growth_probs = col_f64(&food_sat_by_tick, "avg_growth_prob");

    // Show phases
    let phases = [(0, 20, "early"), (20, 100, "mid"), (100, 200, "late"), (200, 300, "final")];
    for (start, end, label) in &phases {
        let sats: Vec<f64> = avg_sats
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        let gps: Vec<f64> = avg_growth_probs
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        if !sats.is_empty() {
            println!(
                "  {label:>6} (t={start}-{end}): avg_food_sat={:.4} avg_growth_prob={:.6}",
                mean(&sats),
                mean(&gps)
            );
        }
    }

    // ── CLAIM 3: Merchant runs out of goods during crisis, can't sell to pops ──
    println!("\n--- CLAIM 3: Merchant stock and production timeline ---");
    let stock_flow = dfs.get("stock_flow_good").expect("stock_flow_good dataframe");
    let merchant_goods_by_tick = stock_flow
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([col("goods_after").sum().alias("total_goods")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let total_goods_series = col_f64(&merchant_goods_by_tick, "total_goods");

    let production_io = dfs.get("production_io").expect("production_io dataframe");
    let output_by_tick = production_io
        .clone()
        .lazy()
        .filter(col("direction").eq(lit("output")))
        .group_by([col("tick")])
        .agg([col("quantity").sum().alias("total_output")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let output_series = col_f64(&output_by_tick, "total_output");

    for (start, end, label) in &phases {
        let goods: Vec<f64> = total_goods_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        let output: Vec<f64> = output_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        if !goods.is_empty() {
            println!(
                "  {label:>6} (t={start}-{end}): avg_total_goods={:.2} avg_production={:.2}",
                mean(&goods),
                mean(&output)
            );
        }
    }

    // ── CLAIM 4: Merchant currency is depleted, can't pay wages ──
    println!("\n--- CLAIM 4: Merchant currency timeline ---");
    let stock_flow_currency = dfs.get("stock_flow").expect("stock_flow dataframe");
    let merchant_currency_by_tick = stock_flow_currency
        .clone()
        .lazy()
        .select([col("tick"), col("merchant_currency_after")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let merchant_currency_series = col_f64(&merchant_currency_by_tick, "merchant_currency_after");

    for (start, end, label) in &phases {
        let vals: Vec<f64> = merchant_currency_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        if !vals.is_empty() {
            println!(
                "  {label:>6} (t={start}-{end}): avg_merchant_currency={:.2} min={:.2} max={:.2}",
                mean(&vals),
                vals.iter().cloned().fold(f64::INFINITY, f64::min),
                vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            );
        }
    }

    // ── CLAIM 5: Employment drops and doesn't recover ──
    println!("\n--- CLAIM 5: Employment timeline ---");
    let assignment = dfs.get("assignment").expect("assignment dataframe");
    let employed_by_tick = assignment
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([
            col("pop_id").count().alias("employed"),
            col("wage").mean().alias("avg_wage"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let employed_series: Vec<f64> = employed_by_tick
        .column("employed")
        .unwrap()
        .u32()
        .unwrap()
        .into_no_null_iter()
        .map(|v| v as f64)
        .collect();
    let wage_series = col_f64(&employed_by_tick, "avg_wage");

    for (start, end, label) in &phases {
        let emps: Vec<f64> = employed_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        let wages: Vec<f64> = wage_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        if !emps.is_empty() {
            println!(
                "  {label:>6} (t={start}-{end}): avg_employed={:.1} avg_wage={:.4}",
                mean(&emps),
                mean(&wages),
            );
        }
    }

    // ── CLAIM 6: Pop income_ema and buying power collapse ──
    // If income_ema decays during unemployment and doesn't recover,
    // pops can't afford to buy food even when merchant has stock.
    println!("\n--- CLAIM 6: Pop buying behavior ---");
    let order = dfs.get("order").expect("order dataframe");
    let pop_buy_orders_by_tick = order
        .clone()
        .lazy()
        .filter(
            col("agent_type")
                .eq(lit("pop"))
                .and(col("side").eq(lit("buy"))),
        )
        .group_by([col("tick")])
        .agg([
            col("quantity").sum().alias("total_buy_qty"),
            col("limit_price").mean().alias("avg_buy_price"),
            col("order_id").count().alias("buy_order_count"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let buy_qty_series = col_f64(&pop_buy_orders_by_tick, "total_buy_qty");
    let buy_price_series = col_f64(&pop_buy_orders_by_tick, "avg_buy_price");

    for (start, end, label) in &phases {
        let qtys: Vec<f64> = buy_qty_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        let prices: Vec<f64> = buy_price_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        if !qtys.is_empty() {
            println!(
                "  {label:>6} (t={start}-{end}): avg_pop_buy_qty={:.3} avg_buy_limit_price={:.4}",
                mean(&qtys),
                mean(&prices),
            );
        }
    }

    // ── CLAIM 7: Merchant sell orders dry up ──
    println!("\n--- CLAIM 7: Merchant sell behavior ---");
    let merchant_sell_by_tick = order
        .clone()
        .lazy()
        .filter(
            col("agent_type")
                .eq(lit("merchant"))
                .and(col("side").eq(lit("sell"))),
        )
        .group_by([col("tick")])
        .agg([
            col("quantity").sum().alias("total_sell_qty"),
            col("limit_price").mean().alias("avg_sell_price"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let sell_qty_series = col_f64(&merchant_sell_by_tick, "total_sell_qty");
    let sell_price_series = col_f64(&merchant_sell_by_tick, "avg_sell_price");

    for (start, end, label) in &phases {
        let qtys: Vec<f64> = sell_qty_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        let prices: Vec<f64> = sell_price_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        if !qtys.is_empty() {
            println!(
                "  {label:>6} (t={start}-{end}): avg_merchant_sell_qty={:.3} avg_sell_limit_price={:.4}",
                mean(&qtys),
                mean(&prices),
            );
        }
    }

    // ── CLAIM 8: Trade fills show goods not reaching pops ──
    println!("\n--- CLAIM 8: Trade fill rates ---");
    let fill = dfs.get("fill").expect("fill dataframe");
    let pop_buy_fills_by_tick = fill
        .clone()
        .lazy()
        .filter(
            col("agent_type")
                .eq(lit("pop"))
                .and(col("side").eq(lit("buy"))),
        )
        .group_by([col("tick")])
        .agg([
            col("quantity").sum().alias("fill_qty"),
            col("price").mean().alias("fill_price"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let fill_qty_series = col_f64(&pop_buy_fills_by_tick, "fill_qty");
    let fill_price_series = col_f64(&pop_buy_fills_by_tick, "fill_price");

    for (start, end, label) in &phases {
        let qtys: Vec<f64> = fill_qty_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        let prices: Vec<f64> = fill_price_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        if !qtys.is_empty() {
            println!(
                "  {label:>6} (t={start}-{end}): avg_pop_buy_fill_qty={:.3} avg_fill_price={:.4}",
                mean(&qtys),
                mean(&prices),
            );
        }
    }

    // ── CLAIM 9: Consumption vs desired shows unmet demand ──
    println!("\n--- CLAIM 9: Consumption fulfillment ---");
    let consumption_by_tick = consumption
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([
            col("desired").sum().alias("total_desired"),
            col("actual").sum().alias("total_actual"),
            col("stock_before").mean().alias("avg_stock_before"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let desired_series = col_f64(&consumption_by_tick, "total_desired");
    let actual_series = col_f64(&consumption_by_tick, "total_actual");
    let stock_before_series = col_f64(&consumption_by_tick, "avg_stock_before");

    for (start, end, label) in &phases {
        let desired: Vec<f64> = desired_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        let actual: Vec<f64> = actual_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        let stocks: Vec<f64> = stock_before_series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= *start && *i < *end)
            .map(|(_, &v)| v)
            .collect();
        if !desired.is_empty() {
            let fulfill_rate = if mean(&desired) > 0.0 {
                mean(&actual) / mean(&desired)
            } else {
                0.0
            };
            println!(
                "  {label:>6} (t={start}-{end}): avg_desired={:.3} avg_actual={:.3} fulfill={:.4} avg_stock_before={:.4}",
                mean(&desired),
                mean(&actual),
                fulfill_rate,
                mean(&stocks),
            );
        }
    }

    // ── CLAIM 10: Subsistence is or isn't providing food floor ──
    println!("\n--- CLAIM 10: Subsistence allocation ---");
    if let Some(subsistence) = dfs.get("subsistence") {
        let sub_by_tick = subsistence
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("total_subsistence"),
                col("pop_id").count().alias("recipients"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let sub_qty_series = col_f64(&sub_by_tick, "total_subsistence");
        let sub_recipients: Vec<f64> = sub_by_tick
            .column("recipients")
            .unwrap()
            .u32()
            .unwrap()
            .into_no_null_iter()
            .map(|v| v as f64)
            .collect();

        for (start, end, label) in &phases {
            let qtys: Vec<f64> = sub_qty_series
                .iter()
                .enumerate()
                .filter(|(i, _)| *i >= *start && *i < *end)
                .map(|(_, &v)| v)
                .collect();
            let recs: Vec<f64> = sub_recipients
                .iter()
                .enumerate()
                .filter(|(i, _)| *i >= *start && *i < *end)
                .map(|(_, &v)| v)
                .collect();
            if !qtys.is_empty() {
                println!(
                    "  {label:>6} (t={start}-{end}): avg_subsistence_qty={:.4} avg_recipients={:.1}",
                    mean(&qtys),
                    mean(&recs),
                );
            }
        }
    } else {
        println!("  No subsistence dataframe (subsistence disabled or no events)");
    }

    // ── Summary: what's the population at each phase? ──
    println!("\n--- Population timeline ---");
    let pop_ticks = col_f64(&pop_count_by_tick, "tick");
    let pop_counts: Vec<f64> = pop_count_by_tick
        .column("pop_count")
        .unwrap()
        .u32()
        .unwrap()
        .into_no_null_iter()
        .map(|v| v as f64)
        .collect();
    for (start, end, label) in &phases {
        let pops: Vec<f64> = pop_ticks
            .iter()
            .zip(pop_counts.iter())
            .filter(|&(&t, _)| t >= *start as f64 && t < *end as f64)
            .map(|(_, &p)| p)
            .collect();
        if !pops.is_empty() {
            println!(
                "  {label:>6} (t={start}-{end}): avg_pop={:.1} min={:.0} max={:.0}",
                mean(&pops),
                pops.iter().cloned().fold(f64::INFINITY, f64::min),
                pops.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            );
        }
    }

    println!("\n{}", "=".repeat(70));
    println!("Key diagnostic: if production is sufficient at the trapped population");
    println!("but food_satisfaction stays near 1.0 (not above), growth_prob stays");
    println!("near zero and population cannot recover. The question is: why isn't");
    println!("satisfaction rising above 1.0 when there's production surplus?");
    println!("{}", "=".repeat(70));
}

/// Deep investigation of feedback loops in the population trap.
///
/// Tests these hypotheses:
/// H1: Both pop bids and merchant asks anchor to price_EMA → clearing ≈ EMA → EMA self-reinforcing
/// H2: Wages track grain price → real purchasing power flat → pops can only afford ~1 unit
/// H3: Pop stock stays low → capped_actual_stocks limits consumption to subsistence floor
/// H4: The merchant sell curve increases quantity but NOT price → no actual downward price pressure
/// H5: There is no mechanism to transfer merchant surplus to pops
#[test]
#[ignore = "analytical investigation; run manually"]
fn investigate_feedback_loops() {
    use sim_core::labor::SubsistenceReservationConfig;

    let initial_price = 2.0;
    let initial_pop_stock = 2.0;
    let initial_merchant_stock = 1.0;
    let num_pops = 100;
    let num_facilities = 2;
    let production_rate = 1.05;
    let ticks = 300;

    let (mut world, settlement) = create_world(
        num_pops,
        num_facilities,
        initial_price,
        initial_pop_stock,
        initial_merchant_stock,
    );

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
            transport_bps: 9000.0,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );
    world.set_external_market(external);
    world.set_subsistence_reservation(SubsistenceReservationConfig::new(GRAIN, 1.5, 10, 10.0));

    let recipes = vec![make_grain_recipe(production_rate)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);

    let mut rec = ScopedRecorder::new("data/investigation", "feedback_loops");
    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);
    }
    let dfs = rec.get();

    println!("\n{}", "=".repeat(80));
    println!("=== FEEDBACK LOOP INVESTIGATION ===");
    println!("{}\n", "=".repeat(80));

    let fill = dfs.get("fill").expect("fill dataframe");
    let order = dfs.get("order").expect("order dataframe");
    let assignment = dfs.get("assignment").expect("assignment dataframe");
    let consumption = dfs.get("consumption").expect("consumption dataframe");
    let mortality = dfs.get("mortality").expect("mortality dataframe");
    let stock_flow = dfs.get("stock_flow").expect("stock_flow dataframe");
    let stock_flow_good = dfs.get("stock_flow_good").expect("stock_flow_good dataframe");

    // ── H1: Price EMA is self-reinforcing ──
    // Reconstruct the price EMA from clearing prices
    println!("--- H1: CLEARING PRICE vs RECONSTRUCTED EMA ---");
    println!("If clearing ≈ EMA every tick, the EMA is self-reinforcing.\n");

    let clearing_by_tick = fill
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([
            // Volume-weighted clearing price
            (col("price") * col("quantity")).sum().alias("pq"),
            col("quantity").sum().alias("q"),
        ])
        .with_column((col("pq") / col("q")).alias("vwap"))
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let clearing_ticks = col_f64(&clearing_by_tick, "tick");
    let clearing_prices = col_f64(&clearing_by_tick, "vwap");

    // Reconstruct EMA (initial = 2.0, alpha = 0.3)
    let mut reconstructed_ema = Vec::with_capacity(clearing_prices.len());
    let mut ema = initial_price;
    for &cp in &clearing_prices {
        ema = 0.7 * ema + 0.3 * cp;
        reconstructed_ema.push(ema);
    }

    // Show at key ticks
    println!("  {:>6} {:>10} {:>10} {:>10}", "tick", "clearing", "ema", "clear/ema");
    let sample_ticks = [0, 5, 10, 15, 20, 30, 50, 75, 100, 150, 200, 250, 299];
    for &t in &sample_ticks {
        if let Some(idx) = clearing_ticks.iter().position(|&x| x as usize == t) {
            let cp = clearing_prices[idx];
            let em = reconstructed_ema[idx];
            println!("  {:>6} {:>10.4} {:>10.4} {:>10.4}", t, cp, em, cp / em);
        }
    }

    // How much does clearing deviate from EMA on average?
    let late_ratios: Vec<f64> = clearing_ticks
        .iter()
        .zip(clearing_prices.iter())
        .zip(reconstructed_ema.iter())
        .filter(|&((&t, _), _)| t >= 50.0)
        .map(|((_, &cp), &em)| cp / em)
        .collect();
    if !late_ratios.is_empty() {
        println!("\n  Late-phase (t>=50) clearing/EMA: mean={:.4} std={:.4}",
            mean(&late_ratios),
            {
                let m = mean(&late_ratios);
                (late_ratios.iter().map(|x| (x - m).powi(2)).sum::<f64>() / late_ratios.len() as f64).sqrt()
            }
        );
    }

    // ── H2: Wages vs grain price ──
    println!("\n--- H2: WAGES vs GRAIN PRICE (real purchasing power) ---");
    println!("If wage/price ≈ 1.0, pop can only afford ~1 unit of grain.\n");

    let wages_by_tick = assignment
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([
            col("wage").mean().alias("avg_wage"),
            col("wage").sum().alias("total_wages"),
            col("pop_id").count().alias("employed"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let avg_wages = col_f64(&wages_by_tick, "avg_wage");
    let wage_ticks = col_f64(&wages_by_tick, "tick");

    println!("  {:>6} {:>10} {:>10} {:>10} {:>10}", "tick", "avg_wage", "clearing_p", "w/p ratio", "units_afford");
    for &t in &sample_ticks {
        let w_idx = wage_ticks.iter().position(|&x| x as usize == t);
        let c_idx = clearing_ticks.iter().position(|&x| x as usize == t);
        if let (Some(wi), Some(ci)) = (w_idx, c_idx) {
            let w = avg_wages[wi];
            let p = clearing_prices[ci];
            println!("  {:>6} {:>10.4} {:>10.4} {:>10.4} {:>10.4}",
                t, w, p, w / p, w / p);
        }
    }

    // ── H3: Pop stock level and consumption cap ──
    println!("\n--- H3: POP STOCK vs CONSUMPTION CAP ---");
    println!("capped_actual_stocks limits consumption based on stock/target ratio.");
    println!("target = desired_ema * 5.0. If stock/target < 0.6, surplus release ≈ 0.\n");

    let stock_by_tick = consumption
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([
            col("stock_before").mean().alias("avg_stock"),
            col("stock_before").min().alias("min_stock"),
            col("stock_before").max().alias("max_stock"),
            col("desired").mean().alias("avg_desired"),
            col("actual").mean().alias("avg_actual"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let avg_stocks = col_f64(&stock_by_tick, "avg_stock");
    let avg_desired = col_f64(&stock_by_tick, "avg_desired");
    let avg_actual = col_f64(&stock_by_tick, "avg_actual");
    let stock_ticks = col_f64(&stock_by_tick, "tick");

    // For the capped_actual_stocks analysis, we need desired_ema.
    // We can approximate: desired_ema is a slow-moving average of the desired consumption.
    // Let's compute: target = desired_ema * 5, norm_c = stock / target
    // We'll use a running EMA of desired as a proxy
    let mut desired_ema_proxy = 1.0_f64; // initial
    let mut computed_caps = Vec::new();
    for i in 0..avg_stocks.len() {
        let stock = avg_stocks[i];
        let desired = avg_desired[i];
        desired_ema_proxy = 0.8 * desired_ema_proxy + 0.2 * desired;
        let target = desired_ema_proxy * 5.0;
        let norm_c = if target > 0.0 { stock / target } else { 1.0 };
        // surplus_release_factor computation
        let span = 1.4 - 0.6; // SURPLUS_RELEASE_RATIO_HIGH - LOW
        let t = ((norm_c - 0.6) / span).clamp(0.0, 1.0);
        let release = t.powf(1.5); // GAMMA = 1.5
        let baseline_floor = 1.0_f64.max(desired_ema_proxy); // subsistence or desired_tick
        let cap = if stock <= baseline_floor {
            stock
        } else {
            baseline_floor + release * (stock - baseline_floor)
        };
        computed_caps.push((target, norm_c, release, cap));
    }

    println!("  {:>6} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "tick", "stock", "des_ema", "target", "norm_c", "release", "cap", "actual");
    for &t in &sample_ticks {
        if let Some(idx) = stock_ticks.iter().position(|&x| x as usize == t) {
            let (target, norm_c, release, cap) = computed_caps[idx];
            println!("  {:>6} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3}",
                t, avg_stocks[idx], target / 5.0, target, norm_c, release, cap, avg_actual[idx]);
        }
    }

    // ── H4: Merchant sell curve structure ──
    println!("\n--- H4: MERCHANT SELL CURVE — QUANTITY vs PRICE ---");
    println!("Does the merchant offer more at LOWER prices when overstocked?");
    println!("Or just more at ALL prices (no real downward pressure)?\n");

    let merchant_sell_detail = order
        .clone()
        .lazy()
        .filter(
            col("agent_type")
                .eq(lit("merchant"))
                .and(col("side").eq(lit("sell"))),
        )
        .collect()
        .unwrap();

    // Compare sell order structure at early vs late ticks
    for (phase_name, t_start, t_end) in [("early(5-15)", 5.0, 15.0), ("late(200-250)", 200.0, 250.0)] {
        let phase = merchant_sell_detail
            .clone()
            .lazy()
            .filter(col("tick").gt_eq(lit(t_start)).and(col("tick").lt(lit(t_end))))
            .group_by([col("limit_price")])
            .agg([
                col("quantity").mean().alias("avg_qty"),
                col("order_id").count().alias("n"),
            ])
            .sort(["limit_price"], Default::default())
            .collect()
            .unwrap();
        let prices = col_f64(&phase, "limit_price");
        let qtys = col_f64(&phase, "avg_qty");
        println!("  {phase_name}: sell orders by limit_price");
        for (i, (&p, &q)) in prices.iter().zip(qtys.iter()).enumerate() {
            if i < 12 {
                println!("    price={:.4}  avg_qty={:.2}", p, q);
            }
        }
        println!();
    }

    // ── H5: Merchant surplus extraction ──
    println!("--- H5: MERCHANT CURRENCY FLOW (surplus extraction) ---");
    println!("If merchant revenue (grain sales) > cost (wages), surplus stays with merchant.\n");

    let merchant_currency = col_f64(&stock_flow, "merchant_currency_after");
    let mc_ticks = col_f64(&stock_flow, "tick");
    let merchant_goods = col_f64(&stock_flow_good, "goods_after");
    let mg_ticks = col_f64(&stock_flow_good, "tick");
    let total_wages_series = col_f64(&wages_by_tick, "total_wages");

    // Revenue per tick = total fill value for merchant sells
    let merchant_revenue_by_tick = fill
        .clone()
        .lazy()
        .filter(
            col("agent_type")
                .eq(lit("merchant"))
                .and(col("side").eq(lit("sell"))),
        )
        .group_by([col("tick")])
        .agg([(col("price") * col("quantity")).sum().alias("revenue")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let revenue_ticks = col_f64(&merchant_revenue_by_tick, "tick");
    let revenues = col_f64(&merchant_revenue_by_tick, "revenue");

    println!("  {:>6} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "tick", "m_currency", "m_stock", "revenue", "wages", "net_flow");
    for &t in &sample_ticks {
        let mc = mc_ticks.iter().position(|&x| x as usize == t).map(|i| merchant_currency[i]);
        let mg = mg_ticks.iter().position(|&x| x as usize == t).map(|i| merchant_goods[i]);
        let rev = revenue_ticks.iter().position(|&x| x as usize == t).map(|i| revenues[i]);
        let wag = wage_ticks.iter().position(|&x| x as usize == t).map(|i| total_wages_series[i]);
        if let (Some(c), Some(g), Some(r), Some(w)) = (mc, mg, rev, wag) {
            println!("  {:>6} {:>10.2} {:>10.2} {:>10.4} {:>10.4} {:>10.4}",
                t, c, g, r, w, r - w);
        }
    }

    // ── H6: Pop bid structure (are pops reducing bids?) ──
    println!("\n--- H6: POP BID STRUCTURE ---");
    println!("Are pop bids falling? What's the bid range relative to clearing?\n");

    let pop_bid_detail = order
        .clone()
        .lazy()
        .filter(
            col("agent_type")
                .eq(lit("pop"))
                .and(col("side").eq(lit("buy"))),
        )
        .collect()
        .unwrap();

    for (phase_name, t_start, t_end) in [("early(5-15)", 5.0, 15.0), ("late(200-250)", 200.0, 250.0)] {
        let phase_stats = pop_bid_detail
            .clone()
            .lazy()
            .filter(col("tick").gt_eq(lit(t_start)).and(col("tick").lt(lit(t_end))))
            .select([
                col("limit_price").mean().alias("avg_bid"),
                col("limit_price").min().alias("min_bid"),
                col("limit_price").max().alias("max_bid"),
                col("quantity").mean().alias("avg_qty"),
                col("quantity").sum().alias("total_qty"),
            ])
            .collect()
            .unwrap();
        let avg_bid = phase_stats.column("avg_bid").unwrap().f64().unwrap().get(0).unwrap_or(0.0);
        let min_bid = phase_stats.column("min_bid").unwrap().f64().unwrap().get(0).unwrap_or(0.0);
        let max_bid = phase_stats.column("max_bid").unwrap().f64().unwrap().get(0).unwrap_or(0.0);
        let avg_qty = phase_stats.column("avg_qty").unwrap().f64().unwrap().get(0).unwrap_or(0.0);
        println!("  {phase_name}: bid_range=[{min_bid:.4}, {max_bid:.4}] avg_bid={avg_bid:.4} avg_qty_per_order={avg_qty:.4}");
    }

    // ── H7: Budget constraint binding? ──
    println!("\n--- H7: IS THE BUDGET CONSTRAINT BINDING? ---");
    println!("Pop budget = min(income_ema, currency). If budget ≈ clearing_price,");
    println!("pop can only afford ~1 unit. Any less and they starve.\n");

    // We can detect budget constraint from fills:
    // If pop total buy fill ≈ budget / clearing_price, budget is binding
    // If pop total buy fill < that, demand curve is the constraint
    let pop_fill_by_tick = fill
        .clone()
        .lazy()
        .filter(
            col("agent_type")
                .eq(lit("pop"))
                .and(col("side").eq(lit("buy"))),
        )
        .group_by([col("tick")])
        .agg([
            col("quantity").sum().alias("total_fill_qty"),
            (col("quantity") * col("price")).sum().alias("total_spent"),
            col("price").mean().alias("avg_fill_price"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let fill_qty_series = col_f64(&pop_fill_by_tick, "total_fill_qty");
    let total_spent_series = col_f64(&pop_fill_by_tick, "total_spent");
    let fill_ticks = col_f64(&pop_fill_by_tick, "tick");

    // Pop count from mortality
    let pop_count_by_tick = mortality
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([col("pop_id").count().alias("pop_count")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();
    let pop_counts: Vec<f64> = pop_count_by_tick
        .column("pop_count")
        .unwrap()
        .u32()
        .unwrap()
        .into_no_null_iter()
        .map(|v| v as f64)
        .collect();
    let pop_ticks = col_f64(&pop_count_by_tick, "tick");

    println!("  {:>6} {:>6} {:>10} {:>10} {:>10} {:>10}",
        "tick", "pops", "fill_qty", "spent", "per_pop_qty", "per_pop_$");
    for &t in &sample_ticks {
        let fi = fill_ticks.iter().position(|&x| x as usize == t);
        let pi = pop_ticks.iter().position(|&x| x as usize == t);
        if let (Some(fi), Some(pi)) = (fi, pi) {
            let pops = pop_counts[pi];
            let qty = fill_qty_series[fi];
            let spent = total_spent_series[fi];
            println!("  {:>6} {:>6.0} {:>10.3} {:>10.4} {:>10.4} {:>10.4}",
                t, pops, qty, spent, qty / pops.max(1.0), spent / pops.max(1.0));
        }
    }

    // ── SYNTHESIS ──
    println!("\n{}", "=".repeat(80));
    println!("=== SYNTHESIS: WHERE DOES THE FEEDBACK LOOP BREAK? ===");
    println!("{}\n", "=".repeat(80));

    // Compute late-phase averages
    let phases = [(100, 300, "trapped (t=100-300)")];
    for (start, end, label) in &phases {
        let late_wages: Vec<f64> = wage_ticks.iter().zip(avg_wages.iter())
            .filter(|&(&t, _)| t >= *start as f64 && t < *end as f64)
            .map(|(_, &w)| w)
            .collect();
        let late_clearing: Vec<f64> = clearing_ticks.iter().zip(clearing_prices.iter())
            .filter(|&(&t, _)| t >= *start as f64 && t < *end as f64)
            .map(|(_, &p)| p)
            .collect();
        let late_stock: Vec<f64> = stock_ticks.iter().zip(avg_stocks.iter())
            .filter(|&(&t, _)| t >= *start as f64 && t < *end as f64)
            .map(|(_, &s)| s)
            .collect();
        let late_actual: Vec<f64> = stock_ticks.iter().zip(avg_actual.iter())
            .filter(|&(&t, _)| t >= *start as f64 && t < *end as f64)
            .map(|(_, &a)| a)
            .collect();
        let late_fill_per_pop: Vec<f64> = fill_ticks.iter().zip(fill_qty_series.iter())
            .zip(
                fill_ticks.iter().map(|&ft| {
                    pop_ticks.iter().zip(pop_counts.iter())
                        .filter(|&(&pt, _)| pt <= ft)
                        .last()
                        .map(|(_, &c)| c)
                        .unwrap_or(100.0)
                })
            )
            .filter(|&((&t, _), _)| t >= *start as f64 && t < *end as f64)
            .map(|((_, &qty), pops)| qty / pops.max(1.0))
            .collect();

        println!("  {} averages:", label);
        println!("    wage:             {:.4}", mean(&late_wages));
        println!("    clearing price:   {:.4}", mean(&late_clearing));
        println!("    wage/price:       {:.4}", mean(&late_wages) / mean(&late_clearing).max(0.001));
        println!("    pop stock:        {:.4}", mean(&late_stock));
        println!("    actual consumed:  {:.4}", mean(&late_actual));
        println!("    fill per pop:     {:.4}", mean(&late_fill_per_pop));
        println!("    surplus = actual - 1.0: {:.4}", mean(&late_actual) - 1.0);
        let wage_price = mean(&late_wages) / mean(&late_clearing).max(0.001);
        println!();
        println!("  Key ratios:");
        println!("    Units affordable per pop (wage/price): {:.4}", wage_price);
        println!("    Actual units consumed per pop:         {:.4}", mean(&late_actual));
        println!("    Gap (affordable - consumed):           {:.4}", wage_price - mean(&late_actual));
    }

    // ── H8: External anchor draining grain? ──
    println!("\n--- H8: EXTERNAL MARKET FLOWS ---");
    println!("Is the external anchor competing with pops for grain (export drain)?\n");

    if let Some(ext_flow) = dfs.get("external_flow") {
        let _flow_by_tick = ext_flow
            .clone()
            .lazy()
            .group_by([col("tick"), col("flow")])
            .agg([col("quantity").sum().alias("qty")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        println!("  External flow summary:");

        let totals = ext_flow
            .clone()
            .lazy()
            .group_by([col("flow")])
            .agg([
                col("quantity").sum().alias("total_qty"),
                col("quantity").mean().alias("avg_per_tick"),
                col("tick").n_unique().alias("active_ticks"),
            ])
            .collect()
            .unwrap();
        for i in 0..totals.height() {
            let flow_name = totals.column("flow").unwrap().str().unwrap().get(i).unwrap_or("");
            let total = totals.column("total_qty").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
            let avg = totals.column("avg_per_tick").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
            let ticks_active = totals.column("active_ticks").unwrap().u32().unwrap().get(i).unwrap_or(0);
            println!("    {flow_name}: total={total:.2} avg_per_event={avg:.4} active_ticks={ticks_active}");
        }

        // Show external orders
        if let Some(ext_order) = dfs.get("order") {
            let ext_orders = ext_order
                .clone()
                .lazy()
                .filter(col("agent_type").eq(lit("external")))
                .group_by([col("side")])
                .agg([
                    col("quantity").sum().alias("total_qty"),
                    col("limit_price").mean().alias("avg_price"),
                    col("limit_price").min().alias("min_price"),
                    col("limit_price").max().alias("max_price"),
                    col("order_id").count().alias("n_orders"),
                ])
                .collect()
                .unwrap();
            println!("\n  External orders in market:");
            for i in 0..ext_orders.height() {
                let side = ext_orders.column("side").unwrap().str().unwrap().get(i).unwrap_or("");
                let qty = ext_orders.column("total_qty").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
                let avg_p = ext_orders.column("avg_price").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
                let min_p = ext_orders.column("min_price").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
                let max_p = ext_orders.column("max_price").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
                let n: u32 = ext_orders.column("n_orders").unwrap().u32().unwrap().get(i).unwrap_or(0);
                println!("    {side}: total_qty={qty:.2} price_range=[{min_p:.4},{max_p:.4}] avg_price={avg_p:.4} n={n}");
            }
        }

        // External fills (grain leaving/entering the settlement)
        let ext_fills = fill
            .clone()
            .lazy()
            .filter(col("agent_type").eq(lit("external")))
            .group_by([col("side")])
            .agg([
                col("quantity").sum().alias("total_qty"),
                col("price").mean().alias("avg_price"),
                (col("quantity") * col("price")).sum().alias("total_value"),
            ])
            .collect()
            .unwrap();
        println!("\n  External fills (actual trades with external market):");
        for i in 0..ext_fills.height() {
            let side = ext_fills.column("side").unwrap().str().unwrap().get(i).unwrap_or("");
            let qty = ext_fills.column("total_qty").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
            let avg_p = ext_fills.column("avg_price").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
            let val = ext_fills.column("total_value").unwrap().f64().unwrap().get(i).unwrap_or(0.0);
            println!("    {side}: total_qty={qty:.2} avg_price={avg_p:.4} total_value={val:.2}");
        }
    } else {
        println!("  No external_flow dataframe");
    }

    // ── H9: Stock accounting — where does grain go each tick? ──
    println!("\n--- H9: GRAIN ACCOUNTING PER TICK ---");
    println!("production - pop_consumption - external_export = merchant_stock_change\n");

    let production_io = dfs.get("production_io").expect("production_io dataframe");
    let prod_by_tick = production_io
        .clone()
        .lazy()
        .filter(col("direction").eq(lit("output")))
        .group_by([col("tick")])
        .agg([col("quantity").sum().alias("produced")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    let cons_by_tick = consumption
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([col("actual").sum().alias("consumed")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    // External fills per tick (grain leaving the settlement)
    let ext_fill_by_tick = fill
        .clone()
        .lazy()
        .filter(col("agent_type").eq(lit("external")).and(col("side").eq(lit("buy"))))
        .group_by([col("tick")])
        .agg([col("quantity").sum().alias("exported")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    // Join all three
    let accounting = prod_by_tick
        .lazy()
        .join(cons_by_tick.lazy(), [col("tick")], [col("tick")], JoinArgs::new(JoinType::Left))
        .join(ext_fill_by_tick.lazy(), [col("tick")], [col("tick")], JoinArgs::new(JoinType::Left))
        .with_column(col("exported").fill_null(lit(0.0)))
        .with_column(col("consumed").fill_null(lit(0.0)))
        .with_column((col("produced") - col("consumed") - col("exported")).alias("residual"))
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    let acc_ticks = col_f64(&accounting, "tick");
    let acc_produced = col_f64(&accounting, "produced");
    let acc_consumed = col_f64(&accounting, "consumed");
    let acc_exported = col_f64(&accounting, "exported");
    let acc_residual = col_f64(&accounting, "residual");

    println!("  {:>6} {:>10} {:>10} {:>10} {:>10}", "tick", "produced", "consumed", "exported", "residual");
    for &t in &sample_ticks {
        if let Some(idx) = acc_ticks.iter().position(|&x| x as usize == t) {
            println!("  {:>6} {:>10.3} {:>10.3} {:>10.3} {:>10.3}",
                t, acc_produced[idx], acc_consumed[idx], acc_exported[idx], acc_residual[idx]);
        }
    }

    // Late-phase averages
    let late_acc: Vec<(f64, f64, f64, f64)> = acc_ticks.iter()
        .zip(acc_produced.iter().zip(acc_consumed.iter().zip(acc_exported.iter().zip(acc_residual.iter()))))
        .filter(|&(&t, _)| t >= 100.0)
        .map(|(_, (&p, (&c, (&e, &r))))| (p, c, e, r))
        .collect();
    if !late_acc.is_empty() {
        let n = late_acc.len() as f64;
        println!("\n  Late avg (t>=100): produced={:.3} consumed={:.3} exported={:.3} residual={:.3}",
            late_acc.iter().map(|x| x.0).sum::<f64>() / n,
            late_acc.iter().map(|x| x.1).sum::<f64>() / n,
            late_acc.iter().map(|x| x.2).sum::<f64>() / n,
            late_acc.iter().map(|x| x.3).sum::<f64>() / n);
    }

    println!();
    println!("  If wage/price ≈ 1.0 → pop can only afford subsistence → no growth possible");
    println!("  If wage/price > 1.0 but stock is low → consumption cap blocks surplus eating");
    println!("  If wage/price < 1.0 → pop can't even afford subsistence, draws from savings");
    println!("{}", "=".repeat(80));
}

/// Compare monopsony (1 merchant) vs competition (2 merchants) to test whether
/// competition transfers production surplus to workers via wage bidding.
///
/// Setup: 60 pops, production_rate=1.05, 2 facilities (cap 50 each).
/// - Monopsony: both facilities owned by same merchant
/// - Competition: each facility owned by a different merchant
///
/// With correct MVP (1.05 × price) and monopsony-aware bid logic:
/// - Monopsony: employer won't raise bid (all workers already hired) → wage ≈ price
/// - Competition: employers bid against each other → wage → 1.05 × price
#[test]
#[ignore = "analytical investigation; run manually"]
fn investigate_monopsony_vs_competition() {
    use sim_core::SubsistenceReservationConfig;

    let production_rate = 1.05;
    let initial_price = 0.5;
    let initial_pop_stock = 3.0;
    let initial_merchant_stock = 50.0;
    let num_pops = 60;
    let ticks = 300;

    let recipes = vec![make_grain_recipe(production_rate)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);

    println!("\n{}", "=".repeat(80));
    println!("=== MONOPSONY VS COMPETITION INVESTIGATION ===");
    println!("{}", "=".repeat(80));
    println!("  production_rate={production_rate}, pops={num_pops}, 2 facilities (cap 50 each)");
    println!("  Monopsony: 1 merchant owns both facilities");
    println!("  Competition: 2 merchants, each owns 1 facility");
    println!();

    for (label, num_merchants) in [("MONOPSONY", 1), ("COMPETITION", 2)] {
        // Build world with the right merchant structure
        let mut world = World::new();
        let settlement = world.add_settlement("TestTown", (0.0, 0.0));

        let merchants: Vec<_> = (0..num_merchants).map(|_| {
            let m = world.add_merchant();
            {
                let merchant = world.get_merchant_mut(m).unwrap();
                merchant.currency = 10_000.0;
                merchant
                    .stockpile_at(settlement)
                    .add(GRAIN, initial_merchant_stock / num_merchants as f64);
            }
            m
        }).collect();

        // Create facilities — divide ownership across merchants
        let mut facility_ids = Vec::new();
        for i in 0..2 {
            let owner = merchants[i % num_merchants];
            let farm = world
                .add_facility(FacilityType::Farm, settlement, owner)
                .unwrap();
            let f = world.get_facility_mut(farm).unwrap();
            f.capacity = 50;
            f.recipe_priorities = vec![RecipeId::new(1)];
            facility_ids.push(farm);
        }

        // Create pops
        for i in 0..num_pops {
            let pop = world.add_pop(settlement).unwrap();
            let facility = facility_ids[i % 2];
            let p = world.get_pop_mut(pop).unwrap();
            p.currency = 100.0;
            p.skills.insert(LABORER);
            p.min_wage = 0.01;
            p.employed_at = Some(facility);
            p.income_ema = 0.4;
            p.stocks.insert(GRAIN, initial_pop_stock);
            p.desired_consumption_ema.insert(GRAIN, 1.0);
        }

        // Start wage below MVP so the competitive raise path has room to operate.
        // MVP = production_rate × initial_price = 1.05 × 0.5 = 0.525
        // wage_ema starts at 0.4 (below MVP), so bid starts below MVP.
        world.wage_ema.insert(LABORER, 0.4);
        world.price_ema.insert((settlement, GRAIN), initial_price);

        // Enable subsistence reservation (low reservation wages)
        world.subsistence_reservation = Some(SubsistenceReservationConfig::new(GRAIN, 1.5, 10, 10.0));

        // Light external anchor — don't drain everything
        let mut external = ExternalMarketConfig::default();
        external.anchors.insert(
            GRAIN,
            AnchoredGoodConfig {
                world_price: 10.0,
                spread_bps: 500.0,
                base_depth: 0.0,
                depth_per_pop: 0.05,
                tiers: 9,
                tier_step_bps: 300.0,
            },
        );
        external.frictions.insert(
            settlement,
            SettlementFriction {
                enabled: true,
                transport_bps: 9000.0,
                tariff_bps: 0.0,
                risk_bps: 0.0,
            },
        );
        world.set_external_market(external);

        // Run simulation and collect per-tick data
        let rec_name = format!("monopsony_vs_comp_{}", label.to_lowercase());
        let mut rec = ScopedRecorder::new("data/investigation", &rec_name);
        for _ in 0..ticks {
            world.run_tick(&good_profiles, &needs, &recipes);
        }
        let dfs = rec.get();

        // --- Analyze results ---
        println!("--- {label} ({num_merchants} merchant{}) ---",
            if num_merchants > 1 { "s" } else { "" });

        // Population trajectory
        if let Some(pop_df) = dfs.get("pop_tick") {
            let ticks_col = col_f64(pop_df, "tick");
            let pop_count = col_f64(pop_df, "pop_count");
            let sample_ticks = [0usize, 50, 100, 200, 299];
            print!("  Population: ");
            for &t in &sample_ticks {
                if let Some(pos) = ticks_col.iter().position(|&x| x as usize == t) {
                    print!("t{}={:.0}  ", t, pop_count[pos]);
                }
            }
            println!();
            if let Some(&final_pop) = pop_count.last() {
                println!("  Final pop: {:.0}", final_pop);
            }
        }

        // Wage / price ratio
        if let Some(labor_df) = dfs.get("labor_clearing") {
            let wages = col_f64(labor_df, "clearing_wage");
            let tail_wages = trailing(&wages, 40);
            let avg_wage = mean(tail_wages);
            println!("  Tail avg wage: {:.4}", avg_wage);
        }

        if let Some(market_df) = dfs.get("market_clearing") {
            let prices = col_f64(market_df, "clearing_price");
            let tail_prices = trailing(&prices, 40);
            let avg_price = mean(tail_prices);
            println!("  Tail avg price: {:.4}", avg_price);

            if let Some(labor_df) = dfs.get("labor_clearing") {
                let wages = col_f64(labor_df, "clearing_wage");
                let tail_wages = trailing(&wages, 40);
                let avg_wage = mean(tail_wages);
                let ratio = if avg_price > 0.0 { avg_wage / avg_price } else { 0.0 };
                println!("  Tail wage/price: {:.4} (MVP ceiling = {production_rate})", ratio);
            }
        }

        // Adaptive bid state — check what the bid converged to
        for (fid, bid_state) in &world.facility_bid_states {
            if let Some(&bid) = bid_state.bids.get(&LABORER) {
                let price_ema = world.price_ema.values().next().copied().unwrap_or(1.0);
                println!("  Facility {:?} adaptive_bid={:.4} (price_ema={:.4}, MVP={:.4})",
                    fid.0, bid, price_ema, production_rate * price_ema);
            }
        }

        // Merchant stockpile (did surplus accumulate at the merchant?)
        for (i, &mid) in merchants.iter().enumerate() {
            if let Some(m) = world.get_merchant_mut(mid) {
                let grain = m.stockpile_at(settlement).get(GRAIN);
                println!("  Merchant {i} currency={:.1} grain={:.1}", m.currency, grain);
            }
        }

        // Pop stocks (are workers accumulating food?)
        let pop_stocks: Vec<f64> = world.pops.values()
            .map(|p| p.stocks.get(&GRAIN).copied().unwrap_or(0.0))
            .collect();
        if !pop_stocks.is_empty() {
            let avg = pop_stocks.iter().sum::<f64>() / pop_stocks.len() as f64;
            println!("  Avg pop grain stock: {:.3}", avg);
        }

        println!();
    }

    println!("Expected:");
    println!("  Monopsony: wage/price ≈ 1.0 (employer won't bid up, no competition)");
    println!("  Competition: wage/price → {production_rate} (employers outbid each other toward MVP)");
    println!("{}", "=".repeat(80));
}

/// Investigate why the multi_pop_basic_convergence test loses 92 of 100 pops.
///
/// Setup matches convergence.rs exactly:
/// - 100 pops, 2 facilities (50 cap each), pops start UNEMPLOYED
/// - production_rate=1.05, q_max=1.5, carrying_capacity=10
/// - external anchor + subsistence enabled
///
/// Hypotheses to test:
/// H1: On tick 1, most pops get hired (reservation < MVP for rank 11+)
/// H2: "Filled → lower" drives wages below reservation over time
/// H3: Pops leave facilities when wage < reservation, go to subsistence
/// H4: Subsistence can only feed ~16 pops (food_sat >= 0.9), rest die
/// H5: wage_ema floor erodes as wages fall, removing the bid floor
#[test]
#[ignore = "investigation workflow; run manually"]
fn investigate_convergence_pop_crash() {
    for &production_rate in &[1.05, 2.0] {
        run_convergence_investigation(production_rate);
    }
}

fn run_convergence_investigation(production_rate: f64) {
    use sim_core::labor::SubsistenceReservationConfig;

    let num_pops: usize = 100;
    let num_facilities: usize = 2;
    let initial_price = 1.0;
    let initial_pop_stock = 5.0;
    let initial_merchant_stock = 210.0;
    let ticks = 500;

    // Build world matching convergence.rs create_multi_pop_world
    let mut world = World::new();
    let settlement = world.add_settlement("TestTown", (0.0, 0.0));

    let merchant = world.add_merchant();
    {
        let m = world.get_merchant_mut(merchant).unwrap();
        m.currency = 10_000.0;
        m.stockpile_at(settlement).add(GRAIN, initial_merchant_stock);
    }

    let workers_per_facility = num_pops.div_ceil(num_facilities);
    let mut facility_ids = Vec::new();
    for _ in 0..num_facilities {
        let farm = world
            .add_facility(FacilityType::Farm, settlement, merchant)
            .unwrap();
        let f = world.get_facility_mut(farm).unwrap();
        f.capacity = workers_per_facility as u32;
        f.recipe_priorities = vec![RecipeId::new(1)];
        facility_ids.push(farm);
    }

    // Pops start UNEMPLOYED (matching convergence.rs)
    for _ in 0..num_pops {
        let pop = world.add_pop(settlement).unwrap();
        let p = world.get_pop_mut(pop).unwrap();
        p.currency = 100.0;
        p.skills.insert(LABORER);
        p.min_wage = 0.0;
        p.income_ema = 1.0;
        p.stocks.insert(GRAIN, initial_pop_stock);
        p.desired_consumption_ema.insert(GRAIN, 1.0);
    }

    world.wage_ema.insert(LABORER, 1.0);
    world.price_ema.insert((settlement, GRAIN), initial_price);

    // Enable stabilizers matching convergence.rs
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
            transport_bps: 9000.0,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );
    world.set_external_market(external);
    world.set_subsistence_reservation(SubsistenceReservationConfig::new(GRAIN, 1.5, num_pops / 2, 10.0));

    let recipes = vec![make_grain_recipe(production_rate)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);

    let rec_name = format!("convergence_pr{:.0}", production_rate * 100.0);
    let mut rec = ScopedRecorder::new("data/investigation", &rec_name);
    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);
    }
    let dfs = rec.get();

    // Compute theoretical price floor from external anchor
    let world_price = 10.0;
    let band = (500.0 + 9000.0) / 10_000.0; // spread + transport
    let tier_step = 300.0 / 10_000.0;
    let best_export_price = world_price * (1.0 - band); // tier 0
    let worst_export_price = world_price * (1.0 - band) / (1.0 + tier_step * 8.0); // tier 8
    let k = num_pops / 2;
    let alpha = (1.5 - 1.0) / (k as f64 - 1.0);

    println!("\n{}", "=".repeat(80));
    println!("=== CONVERGENCE INVESTIGATION (production_rate={production_rate}) ===");
    println!("=== 100 pops, 2 fac (monopsony), start unemployed, K={k}, q_max=1.5, min_wage=0 ===");
    println!("  Export price floor: [{worst_export_price:.4}, {best_export_price:.4}]");
    println!("  MVP at price floor: [{:.4}, {:.4}]",
        production_rate * worst_export_price, production_rate * best_export_price);
    println!("  Subsistence q at K/2 unemployed: q({}) = {:.4}",
        k/2 + 1, 1.5 / (1.0 + alpha * (k/2) as f64));
    println!("{}\n", "=".repeat(80));

    let assignment = dfs.get("assignment").expect("assignment df");
    let mortality = dfs.get("mortality").expect("mortality df");

    // ── 1: Employment + population per tick (first 30 ticks detail) ──
    println!("--- 1: EMPLOYMENT & POPULATION (tick-by-tick, first 30 ticks) ---\n");

    let pop_by_tick = mortality
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([
            col("pop_id").count().alias("pop_count"),
            col("food_satisfaction").mean().alias("avg_food_sat"),
            col("food_satisfaction").min().alias("min_food_sat"),
            col("outcome").eq(lit("dies")).cast(DataType::Int32).sum().alias("deaths"),
            col("outcome").eq(lit("grows")).cast(DataType::Int32).sum().alias("grows"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    let emp_by_tick = assignment
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([
            col("pop_id").count().alias("employed"),
            col("wage").mean().alias("avg_wage"),
            col("wage").min().alias("min_wage"),
        ])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    let pop_ticks = col_f64(&pop_by_tick, "tick");
    let pop_counts = col_f64(&pop_by_tick, "pop_count");
    let avg_food_sats = col_f64(&pop_by_tick, "avg_food_sat");
    let min_food_sats = col_f64(&pop_by_tick, "min_food_sat");
    let deaths_series = col_f64(&pop_by_tick, "deaths");
    let grows_series = col_f64(&pop_by_tick, "grows");
    let emp_ticks = col_f64(&emp_by_tick, "tick");
    let emp_counts = col_f64(&emp_by_tick, "employed");
    let avg_wages = col_f64(&emp_by_tick, "avg_wage");

    println!("  {:>4} {:>5} {:>5} {:>8} {:>8} {:>6} {:>6} {:>8}",
        "tick", "pops", "empl", "avg_sat", "min_sat", "death", "grow", "avg_wage");
    for t in 0..30usize {
        let pi = pop_ticks.iter().position(|&x| x as usize == t);
        let ei = emp_ticks.iter().position(|&x| x as usize == t);
        if let Some(pi) = pi {
            let empl = ei.map(|i| emp_counts[i]).unwrap_or(0.0);
            let wage = ei.map(|i| avg_wages[i]).unwrap_or(0.0);
            println!("  {:>4} {:>5.0} {:>5.0} {:>8.4} {:>8.4} {:>6.0} {:>6.0} {:>8.4}",
                t, pop_counts[pi], empl, avg_food_sats[pi], min_food_sats[pi],
                deaths_series[pi], grows_series[pi], wage);
        }
    }

    // ── 2: Labor bids vs asks (first 10 ticks) ──
    println!("\n--- 2: LABOR BIDS vs ASKS (first 10 ticks) ---\n");

    let sample_ticks_full = [0, 5, 10, 20, 50, 100, 150, 200, 250, 300, 350, 400, 450, 499];

    if let Some(labor_bid) = dfs.get("labor_bid") {
        let bids_summary = labor_bid
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("max_wage").mean().alias("avg_bid"),
                col("max_wage").min().alias("min_bid"),
                col("max_wage").max().alias("max_bid"),
                col("mvp").mean().alias("avg_mvp"),
                col("adaptive_bid").mean().alias("avg_adaptive"),
                col("bid_id").count().alias("n_bids"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let bid_ticks = col_f64(&bids_summary, "tick");
        let avg_bids = col_f64(&bids_summary, "avg_bid");
        let avg_mvps = col_f64(&bids_summary, "avg_mvp");
        let avg_adaptive = col_f64(&bids_summary, "avg_adaptive");
        let n_bids = col_f64(&bids_summary, "n_bids");

        println!("  {:>4} {:>8} {:>8} {:>8} {:>6}",
            "tick", "avg_bid", "avg_mvp", "adaptive", "n_bids");
        for &t in &sample_ticks_full {
            if let Some(i) = bid_ticks.iter().position(|&x| x as usize == t) {
                println!("  {:>4} {:>8.4} {:>8.4} {:>8.4} {:>6.0}",
                    t, avg_bids[i], avg_mvps[i], avg_adaptive[i], n_bids[i]);
            }
        }
    } else {
        println!("  No labor_bid dataframe");
    }

    if let Some(labor_ask) = dfs.get("labor_ask") {
        println!();
        let asks_summary = labor_ask
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("min_wage").mean().alias("avg_ask"),
                col("min_wage").min().alias("min_ask"),
                col("min_wage").max().alias("max_ask"),
                col("ask_id").count().alias("n_asks"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let ask_ticks = col_f64(&asks_summary, "tick");
        let avg_asks = col_f64(&asks_summary, "avg_ask");
        let min_asks = col_f64(&asks_summary, "min_ask");
        let max_asks = col_f64(&asks_summary, "max_ask");
        let n_asks = col_f64(&asks_summary, "n_asks");

        println!("  {:>4} {:>8} {:>8} {:>8} {:>6}",
            "tick", "avg_ask", "min_ask", "max_ask", "n_asks");
        for &t in &sample_ticks_full {
            if let Some(i) = ask_ticks.iter().position(|&x| x as usize == t) {
                println!("  {:>4} {:>8.4} {:>8.4} {:>8.4} {:>6.0}",
                    t, avg_asks[i], min_asks[i], max_asks[i], n_asks[i]);
            }
        }
    } else {
        println!("  No labor_ask dataframe");
    }

    // ── 3: Subsistence output per tick ──
    println!("\n--- 3: SUBSISTENCE OUTPUT ---\n");

    if let Some(subsistence) = dfs.get("subsistence") {
        let sub_by_tick = subsistence
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("total_sub"),
                col("quantity").mean().alias("avg_per_worker"),
                col("quantity").min().alias("min_per_worker"),
                col("pop_id").count().alias("recipients"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let sub_ticks = col_f64(&sub_by_tick, "tick");
        let total_sub = col_f64(&sub_by_tick, "total_sub");
        let avg_per = col_f64(&sub_by_tick, "avg_per_worker");
        let min_per = col_f64(&sub_by_tick, "min_per_worker");
        let recipients = col_f64(&sub_by_tick, "recipients");

        println!("  {:>4} {:>6} {:>8} {:>8} {:>8}",
            "tick", "recip", "total", "avg/wkr", "min/wkr");
        for t in 0..30usize {
            if let Some(i) = sub_ticks.iter().position(|&x| x as usize == t) {
                println!("  {:>4} {:>6.0} {:>8.3} {:>8.4} {:>8.4}",
                    t, recipients[i], total_sub[i], avg_per[i], min_per[i]);
            }
        }

        // Phase averages
        let phases = [(0, 20, "early"), (20, 50, "mid"), (50, 100, "mid2"), (100, 200, "late")];
        println!();
        for (start, end, label) in &phases {
            let recs: Vec<f64> = sub_ticks.iter().zip(recipients.iter())
                .filter(|&(&t, _)| t >= *start as f64 && t < *end as f64)
                .map(|(_, &r)| r)
                .collect();
            let mins: Vec<f64> = sub_ticks.iter().zip(min_per.iter())
                .filter(|&(&t, _)| t >= *start as f64 && t < *end as f64)
                .map(|(_, &m)| m)
                .collect();
            if !recs.is_empty() {
                println!("  {label:>6} (t={start}-{end}): avg_recipients={:.1} avg_min_per_worker={:.4}",
                    mean(&recs), mean(&mins));
            }
        }
    } else {
        println!("  No subsistence dataframe");
    }

    // ── 4: Wage trajectory over full run ──
    println!("\n--- 4: WAGE + EMPLOYMENT TRAJECTORY (sampled) ---\n");
    println!("  {:>4} {:>5} {:>5} {:>5} {:>8} {:>8} {:>8}",
        "tick", "pops", "empl", "unemp", "avg_wage", "avg_sat", "deaths");
    for &t in &sample_ticks_full {
        let pi = pop_ticks.iter().position(|&x| x as usize == t);
        let ei = emp_ticks.iter().position(|&x| x as usize == t);
        if let Some(pi) = pi {
            let pops = pop_counts[pi];
            let empl = ei.map(|i| emp_counts[i]).unwrap_or(0.0);
            let unemp = pops - empl;
            let wage = ei.map(|i| avg_wages[i]).unwrap_or(0.0);
            println!("  {:>4} {:>5.0} {:>5.0} {:>5.0} {:>8.4} {:>8.4} {:>8.0}",
                t, pops, empl, unemp, wage, avg_food_sats[pi], deaths_series[pi]);
        }
    }

    // ── 5: Bid adjustment analysis - is wage_ema floor eroding? ──
    println!("\n--- 5: ADAPTIVE BID + WAGE EMA TRAJECTORY ---\n");

    if let Some(labor_bid) = dfs.get("labor_bid") {
        let bid_trajectory = labor_bid
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("adaptive_bid").mean().alias("avg_adaptive"),
                col("mvp").mean().alias("avg_mvp"),
                col("max_wage").mean().alias("avg_actual_bid"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let bt = col_f64(&bid_trajectory, "tick");
        let adaptive = col_f64(&bid_trajectory, "avg_adaptive");
        let mvp = col_f64(&bid_trajectory, "avg_mvp");
        let actual_bid = col_f64(&bid_trajectory, "avg_actual_bid");

        println!("  {:>4} {:>10} {:>10} {:>10}",
            "tick", "adaptive", "actual_bid", "mvp");
        for &t in &sample_ticks_full {
            if let Some(i) = bt.iter().position(|&x| x as usize == t) {
                println!("  {:>4} {:>10.4} {:>10.4} {:>10.4}",
                    t, adaptive[i], actual_bid[i], mvp[i]);
            }
        }
    }

    // ── 6: Skill outcomes - what does the bid adjuster see? ──
    println!("\n--- 6: SKILL OUTCOMES (bid adjuster inputs) ---\n");

    if let Some(skill_outcome) = dfs.get("skill_outcome") {
        println!("  Columns: {:?}", skill_outcome.get_column_names());
        let so_summary = skill_outcome
            .clone()
            .lazy()
            .group_by([col("tick")])
            .agg([
                col("wanted").sum().alias("total_wanted"),
                col("filled").sum().alias("total_filled"),
                col("profitable_unfilled").sum().alias("total_prof_unfilled"),
                col("marginal_mvp").mean().alias("avg_marginal_mvp"),
            ])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap();
        let so_ticks = col_f64(&so_summary, "tick");
        let wanted = col_f64(&so_summary, "total_wanted");
        let filled = col_f64(&so_summary, "total_filled");
        let prof_unfilled = col_f64(&so_summary, "total_prof_unfilled");
        let marginal_mvp = col_f64(&so_summary, "avg_marginal_mvp");

        println!("  {:>4} {:>6} {:>8} {:>12} {:>8}",
            "tick", "wanted", "filled", "prof_unfill", "marg_mvp");
        for &t in &sample_ticks_full {
            if let Some(i) = so_ticks.iter().position(|&x| x as usize == t) {
                println!("  {:>4} {:>6.0} {:>8.0} {:>12.0} {:>8.4}",
                    t, wanted[i], filled[i], prof_unfilled[i], marginal_mvp[i]);
            }
        }
    } else {
        println!("  No skill_outcome dataframe");
    }

    // ── 7: Grain accounting — where does grain come from / go? ──
    println!("\n--- 7: GRAIN ACCOUNTING (production vs consumption vs external) ---\n");

    let fill = dfs.get("fill").expect("fill df");
    let order = dfs.get("order").expect("order df");

    // Production output
    let production_io = dfs.get("production_io");
    let prod_by_tick = production_io.map(|pio| {
        pio.clone()
            .lazy()
            .filter(col("direction").eq(lit("output")))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("produced")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap()
    });

    // Consumption
    let consumption = dfs.get("consumption").expect("consumption df");
    let cons_by_tick = consumption
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([col("actual").sum().alias("consumed")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    // External flows
    let ext_flow = dfs.get("external_flow");
    let ext_import_by_tick = ext_flow.map(|ef| {
        ef.clone()
            .lazy()
            .filter(col("flow").eq(lit("import")))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("imported")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap()
    });
    let ext_export_by_tick = ext_flow.map(|ef| {
        ef.clone()
            .lazy()
            .filter(col("flow").eq(lit("export")))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("exported")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap()
    });

    // Subsistence (already have sub_by_tick data but let's recompute to be safe)
    let sub_total_by_tick = dfs.get("subsistence").map(|s| {
        s.clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("subsistence")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap()
    });

    // Merchant stock
    let stock_flow_good = dfs.get("stock_flow_good");
    let merchant_stock_by_tick = stock_flow_good.map(|sfg| {
        sfg.clone()
            .lazy()
            .group_by([col("tick")])
            .agg([col("goods_after").sum().alias("merchant_grain")])
            .sort(["tick"], Default::default())
            .collect()
            .unwrap()
    });

    // Price EMA (from fill data)
    let price_by_tick = fill
        .clone()
        .lazy()
        .group_by([col("tick")])
        .agg([col("price").mean().alias("clearing_price")])
        .sort(["tick"], Default::default())
        .collect()
        .unwrap();

    println!("  {:>4} {:>6} {:>6} {:>6} {:>6} {:>6} {:>8} {:>8}",
        "tick", "prod", "subs", "cons", "import", "export", "m_grain", "price");
    for &t in &sample_ticks_full {
        let produced = prod_by_tick.as_ref().and_then(|df| {
            let ticks = col_f64(df, "tick");
            ticks.iter().position(|&x| x as usize == t).map(|i| col_f64(df, "produced")[i])
        }).unwrap_or(0.0);
        let subsistence_out = sub_total_by_tick.as_ref().and_then(|df| {
            let ticks = col_f64(df, "tick");
            ticks.iter().position(|&x| x as usize == t).map(|i| col_f64(df, "subsistence")[i])
        }).unwrap_or(0.0);
        let consumed = {
            let ticks = col_f64(&cons_by_tick, "tick");
            ticks.iter().position(|&x| x as usize == t).map(|i| col_f64(&cons_by_tick, "consumed")[i]).unwrap_or(0.0)
        };
        let imported = ext_import_by_tick.as_ref().and_then(|df| {
            let ticks = col_f64(df, "tick");
            ticks.iter().position(|&x| x as usize == t).map(|i| col_f64(df, "imported")[i])
        }).unwrap_or(0.0);
        let exported = ext_export_by_tick.as_ref().and_then(|df| {
            let ticks = col_f64(df, "tick");
            ticks.iter().position(|&x| x as usize == t).map(|i| col_f64(df, "exported")[i])
        }).unwrap_or(0.0);
        let m_grain = merchant_stock_by_tick.as_ref().and_then(|df| {
            let ticks = col_f64(df, "tick");
            ticks.iter().position(|&x| x as usize == t).map(|i| col_f64(df, "merchant_grain")[i])
        }).unwrap_or(0.0);
        let price = {
            let ticks = col_f64(&price_by_tick, "tick");
            ticks.iter().position(|&x| x as usize == t).map(|i| col_f64(&price_by_tick, "clearing_price")[i]).unwrap_or(0.0)
        };
        println!("  {:>4} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>8.2} {:>8.4}",
            t, produced, subsistence_out, consumed, imported, exported, m_grain, price);
    }

    // ── 8: Pop currency + merchant currency ──
    println!("\n--- 8: CURRENCY BALANCES ---\n");

    let stock_flow = dfs.get("stock_flow");
    if let Some(sf) = stock_flow {
        let sf_ticks = col_f64(sf, "tick");
        let mc = col_f64(sf, "merchant_currency_after");
        println!("  {:>4} {:>12}", "tick", "merchant_$");
        for &t in &sample_ticks_full {
            if let Some(i) = sf_ticks.iter().position(|&x| x as usize == t) {
                println!("  {:>4} {:>12.2}", t, mc[i]);
            }
        }
    }

    // ── Summary ──
    println!("\n{}", "=".repeat(80));
    println!("SUMMARY:");
    let final_pop = pop_counts.last().copied().unwrap_or(0.0);
    let final_empl = emp_counts.last().copied().unwrap_or(0.0);
    let total_deaths: f64 = deaths_series.iter().sum();
    let total_grows: f64 = grows_series.iter().sum();
    println!("  Final: {:.0} pops, {:.0} employed, {:.0} unemployed",
        final_pop, final_empl, final_pop - final_empl);
    println!("  Total deaths: {:.0}, Total grows: {:.0}, Net: {:.0}",
        total_deaths, total_grows, total_grows - total_deaths);
    println!("{}", "=".repeat(80));
}

/// Linear regression slope (least squares) of y on x = 0,1,2,...
fn lin_slope(y: &[f64]) -> f64 {
    let n = y.len() as f64;
    if n < 2.0 { return 0.0; }
    let x_mean = (n - 1.0) / 2.0;
    let y_mean = mean(y);
    let num: f64 = y.iter().enumerate()
        .map(|(i, &yi)| (i as f64 - x_mean) * (yi - y_mean))
        .sum();
    let den: f64 = (0..y.len())
        .map(|i| (i as f64 - x_mean).powi(2))
        .sum();
    if den.abs() < 1e-12 { 0.0 } else { num / den }
}

/// 10k-tick analytical investigation: is the 2.0x production economy at equilibrium?
///
/// Instead of printing sampled rows, computes windowed statistics and trend analysis.
/// Hypotheses to test:
///   H1: Price has a stable mean over the last 5000 ticks (slope ≈ 0)
///   H2: Merchant grain stock is bounded (not accumulating indefinitely)
///   H3: Population is stable (not trending up or down)
///   H4: Employment crashes become less frequent over time (or stop)
#[test]
#[ignore = "long-run equilibrium analysis; run manually"]
fn investigate_longrun_equilibrium() {
    use sim_core::labor::SubsistenceReservationConfig;

    let num_pops: usize = 100;
    let num_facilities: usize = 2;
    let production_rate = 2.0;
    let ticks = 10_000;

    // Build world (same setup as convergence investigation)
    let mut world = World::new();
    let settlement = world.add_settlement("TestTown", (0.0, 0.0));

    let merchant = world.add_merchant();
    {
        let m = world.get_merchant_mut(merchant).unwrap();
        m.currency = 10_000.0;
        m.stockpile_at(settlement).add(GRAIN, 210.0);
    }

    let workers_per_facility = num_pops.div_ceil(num_facilities);
    for _ in 0..num_facilities {
        let farm = world.add_facility(FacilityType::Farm, settlement, merchant).unwrap();
        let f = world.get_facility_mut(farm).unwrap();
        f.capacity = workers_per_facility as u32;
        f.recipe_priorities = vec![RecipeId::new(1)];
    }

    for _ in 0..num_pops {
        let pop = world.add_pop(settlement).unwrap();
        let p = world.get_pop_mut(pop).unwrap();
        p.currency = 100.0;
        p.skills.insert(LABORER);
        p.min_wage = 0.0;
        p.income_ema = 1.0;
        p.stocks.insert(GRAIN, 5.0);
        p.desired_consumption_ema.insert(GRAIN, 1.0);
    }

    world.wage_ema.insert(LABORER, 1.0);
    world.price_ema.insert((settlement, GRAIN), 1.0);

    let mut external = ExternalMarketConfig::default();
    external.anchors.insert(GRAIN, AnchoredGoodConfig {
        world_price: 10.0,
        spread_bps: 500.0,
        base_depth: 0.0,
        depth_per_pop: 0.1,
        tiers: 9,
        tier_step_bps: 300.0,
    });
    external.frictions.insert(settlement, SettlementFriction {
        enabled: true,
        transport_bps: 9000.0,
        tariff_bps: 0.0,
        risk_bps: 0.0,
    });
    world.set_external_market(external);
    world.set_subsistence_reservation(SubsistenceReservationConfig::new(GRAIN, 1.5, num_pops / 2, 10.0));

    let recipes = vec![make_grain_recipe(production_rate)];
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);

    let mut rec = ScopedRecorder::new("data/investigation", "longrun_2x");
    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);
    }
    let dfs = rec.get();

    // === Build per-tick summary DataFrame ===
    let mortality = dfs.get("mortality").expect("mortality df");
    let assignment = dfs.get("assignment").expect("assignment df");
    let fill = dfs.get("fill").expect("fill df");

    // Population per tick
    let pop_by_tick = mortality.clone().lazy()
        .group_by([col("tick")])
        .agg([
            col("pop_id").count().alias("pop_count"),
            col("outcome").eq(lit("dies")).cast(DataType::Int32).sum().alias("deaths"),
        ])
        .sort(["tick"], Default::default())
        .collect().unwrap();

    // Employment per tick
    let emp_by_tick = assignment.clone().lazy()
        .group_by([col("tick")])
        .agg([col("pop_id").count().alias("employed")])
        .sort(["tick"], Default::default())
        .collect().unwrap();

    // Price per tick (mean fill price as proxy for clearing price)
    let price_by_tick = fill.clone().lazy()
        .group_by([col("tick")])
        .agg([col("price").mean().alias("price")])
        .sort(["tick"], Default::default())
        .collect().unwrap();

    // Merchant grain per tick
    let merchant_grain_by_tick = dfs.get("stock_flow_good").map(|sfg| {
        sfg.clone().lazy()
            .group_by([col("tick")])
            .agg([col("goods_after").sum().alias("m_grain")])
            .sort(["tick"], Default::default())
            .collect().unwrap()
    });

    // External exports per tick
    let exports_by_tick = dfs.get("external_flow").map(|ef| {
        ef.clone().lazy()
            .filter(col("flow").eq(lit("export")))
            .group_by([col("tick")])
            .agg([col("quantity").sum().alias("exported")])
            .sort(["tick"], Default::default())
            .collect().unwrap()
    });

    // Extract all tick-level vectors
    let pop_ticks = col_f64(&pop_by_tick, "tick");
    let pop_counts = col_f64(&pop_by_tick, "pop_count");
    let deaths_v = col_f64(&pop_by_tick, "deaths");
    let emp_ticks = col_f64(&emp_by_tick, "tick");
    let emp_counts = col_f64(&emp_by_tick, "employed");
    let price_ticks = col_f64(&price_by_tick, "tick");
    let prices = col_f64(&price_by_tick, "price");
    let m_grain_ticks = merchant_grain_by_tick.as_ref().map(|df| col_f64(df, "tick"));
    let m_grain_vals = merchant_grain_by_tick.as_ref().map(|df| col_f64(df, "m_grain"));
    let export_ticks = exports_by_tick.as_ref().map(|df| col_f64(df, "tick"));
    let export_vals = exports_by_tick.as_ref().map(|df| col_f64(df, "exported"));

    // Helper: extract values for ticks in [start, end)
    let window_vals = |ticks: &[f64], vals: &[f64], start: usize, end: usize| -> Vec<f64> {
        ticks.iter().zip(vals.iter())
            .filter(|&(&t, _)| t >= start as f64 && t < end as f64)
            .map(|(_, &v)| v)
            .collect::<Vec<f64>>()
    };

    // For employment: ticks with no assignments show up as missing. Fill with 0.
    let emp_for_tick = |t: usize| -> f64 {
        emp_ticks.iter().position(|&x| x as usize == t)
            .map(|i| emp_counts[i])
            .unwrap_or(0.0)
    };
    let full_emp: Vec<f64> = (1..=ticks).map(|t| emp_for_tick(t)).collect();

    // === 1: WINDOWED STATISTICS ===
    let window_size = 1000;
    let n_windows = ticks / window_size;

    println!("\n{}", "=".repeat(90));
    println!("=== LONG-RUN EQUILIBRIUM ANALYSIS: production_rate={production_rate}, {ticks} ticks ===");
    println!("{}\n", "=".repeat(90));

    println!("--- 1: WINDOWED STATISTICS ({}‐tick windows) ---\n", window_size);
    println!("  {:>10} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "window", "pop_μ", "pop_σ", "empl_μ", "empl_σ", "price_μ", "price_σ", "mgrain_μ", "export_μ");

    for w in 0..n_windows {
        let start = w * window_size + 1;
        let end = (w + 1) * window_size + 1;

        let w_pop = window_vals(&pop_ticks, &pop_counts, start, end);
        let w_emp: Vec<f64> = (start..end).map(|t| emp_for_tick(t)).collect();
        let w_price = window_vals(&price_ticks, &prices, start, end);
        let w_mgrain = m_grain_ticks.as_ref().zip(m_grain_vals.as_ref())
            .map(|(t, v)| window_vals(t, v, start, end))
            .unwrap_or_default();
        let w_export = export_ticks.as_ref().zip(export_vals.as_ref())
            .map(|(t, v)| window_vals(t, v, start, end))
            .unwrap_or_default();

        println!("  {:>5}-{:<4} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8.4} {:>8.4} {:>8.1} {:>8.1}",
            start, end - 1,
            mean(&w_pop), std_dev(&w_pop),
            mean(&w_emp), std_dev(&w_emp),
            mean(&w_price), std_dev(&w_price),
            mean(&w_mgrain), mean(&w_export));
    }

    // === 2: CRASH EVENTS (employment = 0) ===
    println!("\n--- 2: CRASH EVENTS (employment = 0) per window ---\n");
    for w in 0..n_windows {
        let start = w * window_size + 1;
        let end = (w + 1) * window_size + 1;
        let crashes: usize = (start..end).filter(|&t| emp_for_tick(t) < 1.0).count();
        if crashes > 0 {
            println!("  ticks {start}-{}: {crashes} ticks with zero employment", end - 1);
        }
    }
    let total_crashes: usize = (1..=ticks).filter(|&t| emp_for_tick(t) < 1.0).count();
    println!("  Total: {total_crashes} ticks with zero employment out of {ticks}");

    // === 3: TREND ANALYSIS (last 5000 ticks) ===
    let half = ticks / 2;
    println!("\n--- 3: TREND ANALYSIS (last {half} ticks) ---\n");

    let late_prices = window_vals(&price_ticks, &prices, half, ticks + 1);
    let late_pop = window_vals(&pop_ticks, &pop_counts, half, ticks + 1);
    let late_emp: Vec<f64> = (half..=ticks).map(|t| emp_for_tick(t)).collect();
    let late_mgrain = m_grain_ticks.as_ref().zip(m_grain_vals.as_ref())
        .map(|(t, v)| window_vals(t, v, half, ticks + 1))
        .unwrap_or_default();

    let price_slope = lin_slope(&late_prices);
    let pop_slope = lin_slope(&late_pop);
    let emp_slope = lin_slope(&late_emp);
    let mgrain_slope = lin_slope(&late_mgrain);

    println!("  Price:  mean={:.4}, std={:.4}, slope={:.6}/tick ({:.4}/1000 ticks)",
        mean(&late_prices), std_dev(&late_prices), price_slope, price_slope * 1000.0);
    println!("  Pop:    mean={:.1}, std={:.1}, slope={:.4}/tick ({:.2}/1000 ticks)",
        mean(&late_pop), std_dev(&late_pop), pop_slope, pop_slope * 1000.0);
    println!("  Employ: mean={:.1}, std={:.1}, slope={:.4}/tick ({:.2}/1000 ticks)",
        mean(&late_emp), std_dev(&late_emp), emp_slope, emp_slope * 1000.0);
    println!("  M_grain: mean={:.1}, std={:.1}, slope={:.4}/tick ({:.2}/1000 ticks)",
        mean(&late_mgrain), std_dev(&late_mgrain), mgrain_slope, mgrain_slope * 1000.0);

    // === 4: PRICE DISTRIBUTION in last 2000 ticks ===
    let tail_prices = window_vals(&price_ticks, &prices, ticks - 2000, ticks + 1);
    let mut sorted_prices = tail_prices.clone();
    sorted_prices.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p5 = sorted_prices.get(sorted_prices.len() / 20).copied().unwrap_or(0.0);
    let p25 = sorted_prices.get(sorted_prices.len() / 4).copied().unwrap_or(0.0);
    let p50 = sorted_prices.get(sorted_prices.len() / 2).copied().unwrap_or(0.0);
    let p75 = sorted_prices.get(3 * sorted_prices.len() / 4).copied().unwrap_or(0.0);
    let p95 = sorted_prices.get(19 * sorted_prices.len() / 20).copied().unwrap_or(0.0);

    println!("\n--- 4: PRICE DISTRIBUTION (last 2000 ticks) ---\n");
    println!("  p5={p5:.4}  p25={p25:.4}  p50={p50:.4}  p75={p75:.4}  p95={p95:.4}");
    println!("  Export floor: [0.4032, 0.5000]");

    // === 5: EQUILIBRIUM VERDICT ===
    println!("\n--- 5: EQUILIBRIUM VERDICT ---\n");
    let price_drift_per_1k = (price_slope * 1000.0).abs();
    let mgrain_drift_per_1k = (mgrain_slope * 1000.0).abs();
    let pop_drift_per_1k = (pop_slope * 1000.0).abs();

    if price_drift_per_1k < 0.01 && mgrain_drift_per_1k < 5.0 && pop_drift_per_1k < 1.0 && total_crashes == 0 {
        println!("  STABLE EQUILIBRIUM: all trends near zero, no crashes in last half");
    } else {
        if price_drift_per_1k >= 0.01 {
            println!("  PRICE DRIFTING: {:.4}/1000 ticks (direction: {})",
                price_slope * 1000.0,
                if price_slope < 0.0 { "deflating" } else { "inflating" });
        }
        if mgrain_drift_per_1k >= 5.0 {
            println!("  MERCHANT GRAIN {}: {:.1}/1000 ticks",
                if mgrain_slope > 0.0 { "ACCUMULATING" } else { "DRAINING" },
                mgrain_slope * 1000.0);
        }
        if pop_drift_per_1k >= 1.0 {
            println!("  POPULATION {}: {:.1}/1000 ticks",
                if pop_slope > 0.0 { "GROWING" } else { "DECLINING" },
                pop_slope * 1000.0);
        }
        if total_crashes > 0 {
            let late_crashes: usize = (half..=ticks).filter(|&t| emp_for_tick(t) < 1.0).count();
            println!("  CRASHES: {total_crashes} total ({late_crashes} in last half)");
        }
    }

    println!("\n{}", "=".repeat(90));
}

/// Investigate labor market dynamics with backstop subsistence (q_max=1.02).
///
/// Hypothesis: With q_max=1.02 < production_rate=1.05, all pops should prefer
/// formal employment over subsistence. Employment should be ~100% of capacity.
/// If not, the bottleneck is in the bid adjustment margin (only 3%).
#[test]
#[ignore = "investigation: backstop subsistence labor dynamics"]
fn investigate_backstop_subsistence_labor() {
    use sim_core::SubsistenceReservationConfig;

    let num_pops = 100usize;
    let num_facilities = 2usize;
    let production_rate = 1.05;
    let ticks = 400usize;

    let (mut world, settlement) = create_world(
        num_pops,
        num_facilities,
        1.0,    // initial_price
        5.0,    // initial_pop_stock
        210.0,  // initial_merchant_stock
    );

    // Override: start unemployed with min_wage=0 (matching convergence tests)
    for pop in world.pops.values_mut() {
        pop.employed_at = None;
        pop.min_wage = 0.0;
    }
    for facility in world.facilities.values_mut() {
        facility.workers.clear();
    }

    // Configure anchor + subsistence
    configure_anchor(&mut world, settlement, 0.10, 9000.0);
    world.set_subsistence_reservation(SubsistenceReservationConfig::new(GRAIN, 1.0, 50, 10.0));

    let recipes = vec![common::make_grain_recipe(production_rate)];
    let good_profiles = common::make_grain_profile();
    let needs = common::make_food_need(1.0);

    let rec_name = format!("backstop_subsistence_q08_p{num_pops}");
    let mut rec = ScopedRecorder::new("data/investigation", &rec_name);

    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);
    }
    let dfs = rec.get();

    println!("\n=== Merchant Currency Drain Investigation ===");
    println!("  q_max=1.0, K=50, production_rate={production_rate}, pops={num_pops}, capacity=100");
    println!("  Reservation wage = 1.0 * grain_price (subsistence is credible outside option)");
    println!("  Initial: merchant_currency=10000, pop_currency=100*100=10000\n");

    let sample_ticks: Vec<usize> = (0..ticks).step_by(20).chain(std::iter::once(ticks - 1)).collect();

    // === 1. CURRENCY ACCOUNTING: where is the money? ===
    if let Some(stock_flow) = dfs.get("stock_flow") {
        let sf = stock_flow.clone().lazy()
            .sort(["tick"], Default::default())
            .collect().unwrap();

        let ticks_v = col_f64(&sf, "tick");
        let m_cur_before = col_f64(&sf, "merchant_currency_before");
        let m_cur_after = col_f64(&sf, "merchant_currency_after");
        let p_cur_before = col_f64(&sf, "pop_currency_before");
        let p_cur_after = col_f64(&sf, "pop_currency_after");
        let total_before = col_f64(&sf, "currency_before");
        let total_after = col_f64(&sf, "currency_after");

        println!("Currency positions (before → after each tick):");
        println!("  {:>4} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "tick", "merch_cur", "pop_cur", "total_cur", "m_delta", "leak");
        for &t in &sample_ticks {
            if let Some(i) = ticks_v.iter().position(|&x| x as usize == t) {
                let m_delta = m_cur_after[i] - m_cur_before[i];
                let leak = total_after[i] - total_before[i];
                println!("  {:>4} {:>10.1} {:>10.1} {:>10.1} {:>10.2} {:>10.2}",
                    t, m_cur_after[i], p_cur_after[i], total_after[i], m_delta, leak);
            }
        }
    } else {
        println!("  No stock_flow dataframe");
    }

    // === 2. MERCHANT GRAIN: market revenue vs wage cost ===
    // Merchant sell revenue: fills where agent_type=merchant and side=sell
    // Merchant wage cost: sum of wages from assignment df
    if let (Some(fill_df), Some(assignment_df)) = (dfs.get("fill"), dfs.get("assignment")) {
        let merchant_revenue = fill_df.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("sell"))))
            .with_column((col("quantity") * col("price")).alias("revenue"))
            .group_by([col("tick")])
            .agg([
                col("revenue").sum().alias("sell_revenue"),
                col("quantity").sum().alias("grain_sold"),
            ])
            .sort(["tick"], Default::default())
            .collect().unwrap();

        let wage_cost = assignment_df.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("wage").sum().alias("total_wages"),
                col("pop_id").count().alias("employed"),
            ])
            .sort(["tick"], Default::default())
            .collect().unwrap();

        let rev_ticks = col_f64(&merchant_revenue, "tick");
        let revenues = col_f64(&merchant_revenue, "sell_revenue");
        let grain_sold = col_f64(&merchant_revenue, "grain_sold");
        let wage_ticks = col_f64(&wage_cost, "tick");
        let wages = col_f64(&wage_cost, "total_wages");
        let employed = col_f64(&wage_cost, "employed");

        println!("\nMerchant P&L per tick (revenue from grain sales vs wage cost):");
        println!("  {:>4} {:>8} {:>10} {:>10} {:>10} {:>8}",
            "tick", "employed", "wages_out", "grain_rev", "net_P&L", "grain_sold");
        for &t in &sample_ticks {
            let rev = rev_ticks.iter().position(|&x| x as usize == t)
                .map(|i| (revenues[i], grain_sold[i])).unwrap_or((0.0, 0.0));
            let wage = wage_ticks.iter().position(|&x| x as usize == t)
                .map(|i| (wages[i], employed[i])).unwrap_or((0.0, 0.0));
            let net = rev.0 - wage.0;
            println!("  {:>4} {:>8.0} {:>10.2} {:>10.2} {:>10.2} {:>8.1}",
                t, wage.1, wage.0, rev.0, net, rev.1);
        }

        // Also check: is the merchant BUYING grain? (import cost)
        let merchant_buys = fill_df.clone().lazy()
            .filter(col("agent_type").eq(lit("merchant")).and(col("side").eq(lit("buy"))))
            .with_column((col("quantity") * col("price")).alias("cost"))
            .group_by([col("tick")])
            .agg([
                col("cost").sum().alias("buy_cost"),
                col("quantity").sum().alias("grain_bought"),
            ])
            .sort(["tick"], Default::default())
            .collect().unwrap();

        if merchant_buys.height() > 0 {
            let buy_ticks = col_f64(&merchant_buys, "tick");
            let buy_costs = col_f64(&merchant_buys, "buy_cost");
            let grain_bought = col_f64(&merchant_buys, "grain_bought");
            println!("\nMerchant BUY orders (grain purchases from market):");
            println!("  {:>4} {:>10} {:>10}",
                "tick", "grain_bought", "cost");
            for &t in &sample_ticks {
                if let Some(i) = buy_ticks.iter().position(|&x| x as usize == t) {
                    println!("  {:>4} {:>10.2} {:>10.2}",
                        t, grain_bought[i], buy_costs[i]);
                }
            }
        } else {
            println!("\nMerchant made NO buy orders (not purchasing grain on market)");
        }
    }

    // === 3. EXTERNAL FLOWS: where is currency leaving the settlement? ===
    if let Some(ext_flow) = dfs.get("external_flow") {
        let imports = ext_flow.clone().lazy()
            .filter(col("flow").eq(lit("import")))
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("import_qty"),
            ])
            .sort(["tick"], Default::default())
            .collect().unwrap();

        let exports = ext_flow.clone().lazy()
            .filter(col("flow").eq(lit("export")))
            .group_by([col("tick")])
            .agg([
                col("quantity").sum().alias("export_qty"),
            ])
            .sort(["tick"], Default::default())
            .collect().unwrap();

        let imp_ticks = col_f64(&imports, "tick");
        let imp_qty = col_f64(&imports, "import_qty");
        let exp_ticks = col_f64(&exports, "tick");
        let exp_qty = col_f64(&exports, "export_qty");

        println!("\nExternal flows (imports bring grain in + currency out; exports send grain out + currency in):");
        println!("  {:>4} {:>10} {:>10} {:>10}",
            "tick", "import_qty", "export_qty", "net_import");
        for &t in &sample_ticks {
            let imp = imp_ticks.iter().position(|&x| x as usize == t)
                .map(|i| imp_qty[i]).unwrap_or(0.0);
            let exp = exp_ticks.iter().position(|&x| x as usize == t)
                .map(|i| exp_qty[i]).unwrap_or(0.0);
            println!("  {:>4} {:>10.3} {:>10.3} {:>10.3}",
                t, imp, exp, imp - exp);
        }
    } else {
        println!("\n  No external_flow dataframe");
    }

    // === 4. MORTALITY + POPULATION (compact) ===
    if let Some(mortality) = dfs.get("mortality") {
        let mort_by_tick = mortality.clone().lazy()
            .group_by([col("tick")])
            .agg([
                col("pop_id").count().alias("total"),
                col("outcome").filter(col("outcome").eq(lit("dies"))).count().alias("deaths"),
                col("outcome").filter(col("outcome").eq(lit("grows"))).count().alias("births"),
                col("food_satisfaction").mean().alias("avg_sat"),
                col("food_satisfaction").min().alias("min_sat"),
            ])
            .sort(["tick"], Default::default())
            .collect().unwrap();

        let mort_ticks = col_f64(&mort_by_tick, "tick");
        let totals = col_f64(&mort_by_tick, "total");
        let deaths = col_f64(&mort_by_tick, "deaths");
        let births = col_f64(&mort_by_tick, "births");
        let avg_sats = col_f64(&mort_by_tick, "avg_sat");
        let min_sats = col_f64(&mort_by_tick, "min_sat");

        println!("\nPopulation & Mortality:");
        println!("  {:>4} {:>6} {:>6} {:>6} {:>8} {:>8}",
            "tick", "pops", "deaths", "births", "avg_sat", "min_sat");
        for &t in &sample_ticks {
            if let Some(i) = mort_ticks.iter().position(|&x| x as usize == t) {
                println!("  {:>4} {:>6.0} {:>6.0} {:>6.0} {:>8.3} {:>8.3}",
                    t, totals[i], deaths[i], births[i], avg_sats[i], min_sats[i]);
            }
        }
    }
}
