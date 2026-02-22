#[allow(dead_code)]
mod common;
use common::*;

use std::collections::HashMap;

use sim_core::{
    GoodProfile, Need, NeedContribution, Recipe, ResourceQuality, SubsistenceReservationConfig,
    UtilityCurve, World,
};
const SUBSISTENCE_Q_MAX: f64 = 1.5;
const SUBSISTENCE_CARRYING_CAPACITY: usize = 40;
const FOOD_REQUIREMENT: f64 = 1.0;
const FOOD_SURPLUS_CAP_RATIO: f64 = 1.25;
const SUBSISTENCE_RESOURCE_QUALITY: ResourceQuality = ResourceQuality::Normal;

fn subsistence_total_output(pop_count: usize, q_max: f64, carrying_capacity: usize) -> f64 {
    use sim_core::labor::subsistence::subsistence_output_per_worker;
    (1..=pop_count)
        .map(|rank| subsistence_output_per_worker(rank, q_max, carrying_capacity))
        .sum()
}

fn predict_carrying_capacity_from_subsistence(
    q_max: f64,
    carrying_capacity: usize,
    requirement: f64,
    resource_quality_multiplier: f64,
    max_search: usize,
) -> usize {
    let effective_q_max = q_max * resource_quality_multiplier;

    let mut best_n = 1usize;
    let mut best_abs_gap = f64::MAX;
    for n in 1..=max_search.max(1) {
        let produced = subsistence_total_output(n, effective_q_max, carrying_capacity);
        let required = n as f64 * requirement;
        let abs_gap = (produced - required).abs();
        if abs_gap < best_abs_gap {
            best_abs_gap = abs_gap;
            best_n = n;
        }
    }
    best_n
}

fn effective_food_demand_per_pop(requirement: f64) -> f64 {
    // Runtime utility keeps positive MU in the subsistence surplus band
    // from 1.0x..1.25x requirement. A midpoint approximation is a good
    // predictor of long-run consumption under the greedy stock-only pass.
    requirement * (1.0 + FOOD_SURPLUS_CAP_RATIO) * 0.5
}

fn run_subsistence_only_trial(initial_pop: usize, ticks: usize) -> Vec<f64> {
    let mut world = World::new();
    let settlement = world.add_settlement("CarryTown", (0.0, 0.0));

    // No facilities or merchant production; carrying capacity is driven by
    // in-kind subsistence alone with fixed q_max and crowding_alpha.
    world.set_subsistence_reservation(SubsistenceReservationConfig::new(
        GRAIN,
        SUBSISTENCE_Q_MAX,
        SUBSISTENCE_CARRYING_CAPACITY,
        10.0,
        0.10,
    ));

    for _ in 0..initial_pop {
        let pop_id = world
            .add_pop(settlement)
            .expect("pop insertion should succeed");
        let pop = world.get_pop_mut(pop_id).expect("pop must exist");
        pop.currency = 0.0;
        pop.income_ema = 0.0;
        pop.stocks.insert(GRAIN, 1.0);
        // Disable market-order generation so this isolates subsistence demography.
        pop.desired_consumption_ema.insert(GRAIN, 0.0);
    }

    let good_profiles = vec![GoodProfile {
        good: GRAIN,
        contributions: vec![NeedContribution {
            need_id: "food".to_string(),
            efficiency: 1.0,
        }],
    }];

    let mut needs = HashMap::new();
    needs.insert(
        "food".to_string(),
        Need {
            id: "food".to_string(),
            utility_curve: UtilityCurve::Subsistence {
                requirement: FOOD_REQUIREMENT,
                steepness: 5.0,
            },
        },
    );

    let recipes: Vec<Recipe> = Vec::new();
    let mut pop_history = Vec::with_capacity(ticks);
    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);
        pop_history.push(world.pops.len() as f64);
    }

    pop_history
}

#[test]
fn population_converges_to_constant_carrying_capacity_across_initial_sweep() {
    const TICKS: usize = 700;
    const TAIL: usize = 160;
    const REPS: usize = 3;
    let starts = [20usize, 40, 60, 80, 100, 140, 180, 220, 260];
    let effective_requirement = effective_food_demand_per_pop(FOOD_REQUIREMENT);
    let predicted_capacity = predict_carrying_capacity_from_subsistence(
        SUBSISTENCE_Q_MAX,
        SUBSISTENCE_CARRYING_CAPACITY,
        effective_requirement,
        SUBSISTENCE_RESOURCE_QUALITY.multiplier(),
        600,
    ) as f64;

    let mut scenario_tail_means: Vec<(usize, f64)> = Vec::new();
    for start in starts {
        let mut rep_tail_means = Vec::with_capacity(REPS);

        for _ in 0..REPS {
            let history = run_subsistence_only_trial(start, TICKS);
            let tail = &history[TICKS - TAIL..];
            let tail_mean = mean(tail);
            let tail_std = std_dev(tail);

            // Strongly discourage non-convergent runs.
            assert!(
                tail_std <= 10.0,
                "tail instability too high for start_pop={start}: tail_std={tail_std:.3}, tail_mean={tail_mean:.3}"
            );
            assert!(
                tail_mean > 5.0,
                "unexpected near-extinction for start_pop={start}: tail_mean={tail_mean:.3}"
            );

            rep_tail_means.push(tail_mean);
        }

        scenario_tail_means.push((start, mean(&rep_tail_means)));
    }

    let means_only: Vec<f64> = scenario_tail_means.iter().map(|(_, m)| *m).collect();
    let sweep_center = mean(&means_only);
    let sweep_min = means_only.iter().fold(f64::INFINITY, |a, b| a.min(*b));
    let sweep_max = means_only.iter().fold(f64::NEG_INFINITY, |a, b| a.max(*b));
    let sweep_band = sweep_max - sweep_min;

    // "Constant carrying capacity" across wide initial-condition sweep:
    // all starts should settle into a relatively tight tail band.
    assert!(
        sweep_band <= 18.0,
        "carrying-capacity sweep band too wide: band={sweep_band:.3}, min={sweep_min:.3}, max={sweep_max:.3}, center={sweep_center:.3}, points={scenario_tail_means:?}"
    );

    for (start, m) in &scenario_tail_means {
        assert!(
            (m - sweep_center).abs() <= 9.0,
            "start_pop={start} settled too far from sweep center: mean={m:.3}, center={sweep_center:.3}, points={scenario_tail_means:?}"
        );
    }

    // Assert against analytical carrying capacity implied by the subsistence
    // output curve and settlement natural-resource quality.
    assert!(
        (sweep_center - predicted_capacity).abs() <= 30.0,
        "simulated carrying capacity deviates from subsistence/resource prediction: predicted={predicted_capacity:.3}, observed_center={sweep_center:.3}, effective_requirement={effective_requirement:.3}, points={scenario_tail_means:?}"
    );
}
