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

pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted[sorted.len() / 2]
}

/// Coefficient of variation (std / mean). Returns 0 for empty/zero-mean data.
pub fn cv(values: &[f64]) -> f64 {
    let m = mean(values);
    if m.abs() < 1e-15 {
        0.0
    } else {
        std_dev(values) / m
    }
}

/// Linear regression slope (value per index step).
pub fn trend_slope(values: &[f64]) -> f64 {
    let n = values.len();
    if n <= 1 {
        return 0.0;
    }
    let n_f = n as f64;
    let x_mean = (n_f - 1.0) / 2.0;
    let y_mean = mean(values);
    let mut num = 0.0;
    let mut den = 0.0;
    for (i, &y) in values.iter().enumerate() {
        let x = i as f64 - x_mean;
        num += x * (y - y_mean);
        den += x * x;
    }
    if den.abs() < 1e-15 { 0.0 } else { num / den }
}

/// Summary statistics for a trailing window of a time series.
#[derive(Debug, Clone, Copy)]
pub struct TailStats {
    pub mean: f64,
    pub std: f64,
    pub cv: f64,
    pub min: f64,
    pub max: f64,
    pub slope: f64,
    pub median: f64,
}

impl std::fmt::Display for TailStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mean={:.4} std={:.4} cv={:.4} min={:.4} max={:.4} slope={:.6} median={:.4}",
            self.mean, self.std, self.cv, self.min, self.max, self.slope, self.median
        )
    }
}

/// Compute tail statistics over the last `tail_window` values.
pub fn compute_tail_stats(values: &[f64], tail_window: usize) -> TailStats {
    let tail = trailing(values, tail_window);
    TailStats {
        mean: mean(tail),
        std: std_dev(tail),
        cv: cv(tail),
        min: tail.iter().cloned().fold(f64::INFINITY, f64::min),
        max: tail.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        slope: trend_slope(tail),
        median: median(tail),
    }
}

/// Extract a polars DataFrame column as Vec<f64>, handling common numeric types.
pub fn col_f64(df: &polars::prelude::DataFrame, name: &str) -> Vec<f64> {
    let series = df.column(name).unwrap();
    match series.dtype() {
        polars::datatypes::DataType::Float64 => series.f64().unwrap().into_no_null_iter().collect(),
        polars::datatypes::DataType::UInt64 => series
            .u64()
            .unwrap()
            .into_no_null_iter()
            .map(|v| v as f64)
            .collect(),
        polars::datatypes::DataType::UInt32 => series
            .u32()
            .unwrap()
            .into_no_null_iter()
            .map(|v| v as f64)
            .collect(),
        polars::datatypes::DataType::Int32 => series
            .i32()
            .unwrap()
            .into_no_null_iter()
            .map(|v| v as f64)
            .collect(),
        polars::datatypes::DataType::Int64 => series
            .i64()
            .unwrap()
            .into_no_null_iter()
            .map(|v| v as f64)
            .collect(),
        dt => panic!("col_f64: unsupported dtype {dt:?} for column {name}"),
    }
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
