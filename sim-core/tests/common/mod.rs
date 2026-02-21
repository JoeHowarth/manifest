use std::collections::HashMap;

use sim_core::{
    labor::SkillId,
    needs::{Need, UtilityCurve},
    production::{FacilityType, Recipe, RecipeId},
    types::{GoodId, GoodProfile, NeedContribution},
};

// === CONSTANTS ===

pub const GRAIN: GoodId = 1;
pub const LABORER: SkillId = SkillId(1);

// === STATISTICAL HELPERS ===

pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

pub fn std_dev(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let m = mean(values);
    (values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
}

pub fn trailing<T>(values: &[T], n: usize) -> &[T] {
    if values.len() <= n {
        values
    } else {
        &values[values.len() - n..]
    }
}

pub fn variance(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mean = data.iter().sum::<f64>() / data.len() as f64;
    data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / data.len() as f64
}

// === BUILDERS ===

pub fn make_grain_profile() -> Vec<GoodProfile> {
    vec![GoodProfile {
        good: GRAIN,
        contributions: vec![NeedContribution {
            need_id: "food".to_string(),
            efficiency: 1.0,
        }],
    }]
}

pub fn make_food_need(requirement: f64) -> HashMap<String, Need> {
    let mut needs = HashMap::new();
    needs.insert(
        "food".to_string(),
        Need {
            id: "food".to_string(),
            utility_curve: UtilityCurve::Subsistence {
                requirement,
                steepness: 5.0,
            },
        },
    );
    needs
}

pub fn make_grain_recipe(production_rate: f64) -> Recipe {
    Recipe::new(RecipeId::new(1), "Grain Farming", vec![FacilityType::Farm])
        .with_capacity_cost(1)
        .with_worker(LABORER, 1)
        .with_output(GRAIN, production_rate)
}
