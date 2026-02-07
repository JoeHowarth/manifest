use std::collections::HashMap;

use polars::prelude::*;
use sim_core::instrument::ScopedRecorder;
use sim_core::labor::SkillId;
use sim_core::needs::{Need, UtilityCurve};
use sim_core::production::{FacilityType, Recipe, RecipeId};
use sim_core::types::{GoodId, GoodProfile, NeedContribution};
use sim_core::{AnchoredGoodConfig, ExternalMarketConfig, SettlementFriction, World};

const GRAIN: GoodId = 1;
const LABORER: SkillId = SkillId(1);

#[derive(Debug, Clone, Copy)]
struct Scenario {
    name: &'static str,
    depth_per_pop: f64,
    transport_bps: f64,
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

fn make_needs() -> HashMap<String, Need> {
    let mut needs = HashMap::new();
    needs.insert(
        "food".to_string(),
        Need {
            id: "food".to_string(),
            utility_curve: UtilityCurve::Subsistence {
                requirement: 1.0,
                steepness: 5.0,
            },
        },
    );
    needs
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

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn tail(values: &[f64], n: usize) -> &[f64] {
    if values.len() <= n {
        values
    } else {
        &values[values.len() - n..]
    }
}

fn col_f64(df: &DataFrame, name: &str) -> Vec<f64> {
    df.column(name)
        .unwrap()
        .f64()
        .unwrap()
        .into_no_null_iter()
        .collect()
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

    let recipes = vec![make_recipe(1.0)];
    let good_profiles = make_good_profiles();
    let needs = make_needs();

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
        let tail_price = mean(tail(&price_series, 40));

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
        let tail_emp = mean(tail(&emp_series, 40));

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
            mean(tail(&fulfill, 40))
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
            mean(tail(&ratios, 40))
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
