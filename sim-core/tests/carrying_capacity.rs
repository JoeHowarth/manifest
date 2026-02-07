use std::collections::HashMap;

use sim_core::{
    GoodId, GoodProfile, Need, NeedContribution, Recipe, SubsistenceReservationConfig,
    UtilityCurve, World,
};

const GRAIN: GoodId = 1;

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

fn run_subsistence_only_trial(initial_pop: usize, ticks: usize) -> Vec<f64> {
    let mut world = World::new();
    let settlement = world.add_settlement("CarryTown", (0.0, 0.0));

    // No facilities or merchant production; carrying capacity is driven by
    // in-kind subsistence alone with fixed q_max and crowding_alpha.
    world.set_subsistence_reservation(SubsistenceReservationConfig {
        grain_good: GRAIN,
        q_max: 2.0,
        crowding_alpha: 0.02,
        default_grain_price: 10.0,
    });

    for _ in 0..initial_pop {
        let pop_id = world.add_pop(settlement).expect("pop insertion should succeed");
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
                requirement: 1.0,
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

    let mut scenario_tail_means: Vec<(usize, f64)> = Vec::new();
    for start in starts {
        let mut rep_tail_means = Vec::with_capacity(REPS);

        for _ in 0..REPS {
            let history = run_subsistence_only_trial(start, TICKS);
            let tail = &history[TICKS - TAIL..];
            let tail_mean = mean(tail);
            let tail_std = stddev(tail);

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
    let sweep_max = means_only
        .iter()
        .fold(f64::NEG_INFINITY, |a, b| a.max(*b));
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
}
