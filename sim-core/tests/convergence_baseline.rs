use std::collections::HashMap;

use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use sim_core::{
    AnchoredGoodConfig, ExternalMarketConfig, GoodId, GoodProfile, MerchantAgent, MerchantId, Need,
    NeedContribution, Pop, PopId, Price, SettlementFriction, SettlementId, UtilityCurve,
    run_settlement_tick,
};

const GRAIN: GoodId = 1;
const TICKS: usize = 120;
const TAIL_WINDOW: usize = 40;
const SEEDS: [u64; 5] = [3, 7, 19, 42, 2026];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrialMetrics {
    seed: u64,
    final_price: f64,
    tail_price_mean: f64,
    tail_price_std: f64,
    tail_trade_value_mean: f64,
    final_pop_currency_total: f64,
    final_merchant_stock: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AggregateMetrics {
    avg_final_price: f64,
    avg_tail_price_std: f64,
    avg_tail_trade_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConvergenceBaselineSnapshot {
    ticks: usize,
    tail_window: usize,
    trials: Vec<TrialMetrics>,
    aggregate: AggregateMetrics,
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

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn stddev(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mu = mean(xs);
    let var = xs.iter().map(|x| (x - mu).powi(2)).sum::<f64>() / xs.len() as f64;
    var.sqrt()
}

fn run_trial(seed: u64) -> TrialMetrics {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    let settlement = SettlementId::new(0);
    let good_profiles = make_good_profiles();
    let needs = make_needs();

    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, rng.random_range(0.8..1.2));

    // Add a soft grain anchor so this synthetic one-pop baseline does not
    // deterministically decay toward near-zero nominal prices.
    let mut external = ExternalMarketConfig::default();
    external.anchors.insert(
        GRAIN,
        AnchoredGoodConfig {
            world_price: 1.0,
            spread_bps: 500.0,
            base_depth: 2.0,
            depth_per_pop: 1.0,
            tiers: 9,
            tier_step_bps: 300.0,
        },
    );
    external.frictions.insert(
        settlement,
        SettlementFriction {
            enabled: true,
            transport_bps: 0.0,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );

    let mut pop = Pop::new(PopId::new(1), settlement);
    pop.currency = rng.random_range(40.0..80.0);
    pop.income_ema = rng.random_range(0.9..1.2);
    pop.stocks.insert(GRAIN, rng.random_range(2.0..6.0));
    pop.desired_consumption_ema.insert(GRAIN, 1.0);
    let mut pops: Vec<Pop> = vec![pop];

    let mut merchant = MerchantAgent::new(MerchantId::new(9001));
    merchant.currency = rng.random_range(3_000.0..5_000.0);
    merchant
        .stockpile_at(settlement)
        .add(GRAIN, rng.random_range(180.0..260.0));

    let mut merchants = [merchant];
    let mut price_history = Vec::with_capacity(TICKS);
    let mut trade_value_history = Vec::with_capacity(TICKS);

    for tick in 1..=TICKS {
        // Deterministic exogenous flows to keep the market active across the horizon.
        let inflow = 4.5 + rng.random_range(0.0..1.5);
        merchants[0].stockpile_at(settlement).add(GRAIN, inflow);

        for pop in pops.iter_mut() {
            let wage = 0.9;
            pop.currency += wage;
            pop.income_ema = 0.7 * pop.income_ema + 0.3 * wage;
        }

        let mut pop_refs: Vec<&mut Pop> = pops.iter_mut().collect();
        let mut merchant_refs: Vec<&mut MerchantAgent> = merchants.iter_mut().collect();
        let result = run_settlement_tick(
            tick as u64,
            settlement,
            &mut pop_refs,
            &mut merchant_refs,
            &good_profiles,
            &needs,
            &mut price_ema,
            Some(&external),
            None,
            None,
        );

        let price = price_ema.get(&GRAIN).copied().unwrap_or(0.0);
        price_history.push(price);

        // Buy-side fills count each trade once (matching-side fills are duplicated in result).
        let traded_value: f64 = result
            .fills
            .iter()
            .filter(|f| matches!(f.side, sim_core::Side::Buy))
            .map(|f| f.quantity * f.price)
            .sum();
        trade_value_history.push(traded_value);
    }

    let tail_start = TICKS - TAIL_WINDOW;
    let tail_prices = &price_history[tail_start..];
    let tail_trade_value = &trade_value_history[tail_start..];

    let final_pop_currency_total: f64 = pops.iter().map(|p| p.currency).sum();
    let final_merchant_stock = merchants[0]
        .stockpiles
        .get(&settlement)
        .map(|s| s.get(GRAIN))
        .unwrap_or(0.0);

    TrialMetrics {
        seed,
        final_price: *price_history.last().unwrap_or(&0.0),
        tail_price_mean: mean(tail_prices),
        tail_price_std: stddev(tail_prices),
        tail_trade_value_mean: mean(tail_trade_value),
        final_pop_currency_total,
        final_merchant_stock,
    }
}

fn compute_snapshot() -> ConvergenceBaselineSnapshot {
    let trials: Vec<TrialMetrics> = SEEDS.into_iter().map(run_trial).collect();

    let avg_final_price = mean(&trials.iter().map(|t| t.final_price).collect::<Vec<_>>());
    let avg_tail_price_std = mean(&trials.iter().map(|t| t.tail_price_std).collect::<Vec<_>>());
    let avg_tail_trade_value = mean(
        &trials
            .iter()
            .map(|t| t.tail_trade_value_mean)
            .collect::<Vec<_>>(),
    );

    ConvergenceBaselineSnapshot {
        ticks: TICKS,
        tail_window: TAIL_WINDOW,
        trials,
        aggregate: AggregateMetrics {
            avg_final_price,
            avg_tail_price_std,
            avg_tail_trade_value,
        },
    }
}

fn assert_close(name: &str, actual: f64, expected: f64, abs_tol: f64, rel_tol: f64) {
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
fn convergence_summary_matches_saved_baseline() {
    let expected: ConvergenceBaselineSnapshot =
        serde_json::from_str(include_str!("baselines/convergence_baseline.json"))
            .expect("valid convergence baseline JSON");

    let actual = compute_snapshot();

    assert_eq!(actual.ticks, expected.ticks, "tick count changed");
    assert_eq!(
        actual.tail_window, expected.tail_window,
        "tail window changed"
    );
    assert_eq!(
        actual.trials.len(),
        expected.trials.len(),
        "trial count changed"
    );

    for (i, (a, e)) in actual.trials.iter().zip(expected.trials.iter()).enumerate() {
        assert_eq!(a.seed, e.seed, "seed mismatch at trial {i}");

        assert_close(
            &format!("trial[{i}].final_price"),
            a.final_price,
            e.final_price,
            1e-6,
            1e-4,
        );
        assert_close(
            &format!("trial[{i}].tail_price_mean"),
            a.tail_price_mean,
            e.tail_price_mean,
            1e-6,
            1e-4,
        );
        assert_close(
            &format!("trial[{i}].tail_price_std"),
            a.tail_price_std,
            e.tail_price_std,
            1e-6,
            1e-3,
        );
        assert_close(
            &format!("trial[{i}].tail_trade_value_mean"),
            a.tail_trade_value_mean,
            e.tail_trade_value_mean,
            1e-6,
            1e-4,
        );
        assert_close(
            &format!("trial[{i}].final_pop_currency_total"),
            a.final_pop_currency_total,
            e.final_pop_currency_total,
            1e-4,
            1e-4,
        );
        assert_close(
            &format!("trial[{i}].final_merchant_stock"),
            a.final_merchant_stock,
            e.final_merchant_stock,
            1e-4,
            1e-4,
        );
    }

    assert_close(
        "aggregate.avg_final_price",
        actual.aggregate.avg_final_price,
        expected.aggregate.avg_final_price,
        1e-6,
        1e-4,
    );
    assert_close(
        "aggregate.avg_tail_price_std",
        actual.aggregate.avg_tail_price_std,
        expected.aggregate.avg_tail_price_std,
        1e-6,
        1e-3,
    );
    assert_close(
        "aggregate.avg_tail_trade_value",
        actual.aggregate.avg_tail_trade_value,
        expected.aggregate.avg_tail_trade_value,
        1e-6,
        1e-4,
    );
}

#[test]
#[ignore]
fn regenerate_convergence_baseline_snapshot() {
    let snapshot = compute_snapshot();
    println!(
        "{}",
        serde_json::to_string_pretty(&snapshot).expect("serialize snapshot")
    );
}
