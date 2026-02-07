use std::collections::HashMap;

use crate::agents::ConsumptionResult;
use crate::needs::Need;
use crate::types::{GoodId, GoodProfile, Price, Quantity};

// === CONSTANTS ===

const BUFFER_TICKS: f64 = 5.0;
const SURPLUS_RELEASE_RATIO_LOW: f64 = 0.6;
const SURPLUS_RELEASE_RATIO_HIGH: f64 = 1.4;
const SURPLUS_RELEASE_GAMMA: f64 = 1.5;

// === STOCKPILE BIAS ===

/// Compute biased prices for actual consumption based on stockpile vs target.
///
/// - Low stock relative to target → higher virtual price → consume less (save buffer)
/// - High stock relative to target → lower virtual price → consume more (draw down excess)
fn biased_prices(
    stocks: &HashMap<GoodId, Quantity>,
    desired_ema: &HashMap<GoodId, Quantity>,
    base_prices: &HashMap<GoodId, Price>,
) -> HashMap<GoodId, Price> {
    base_prices
        .iter()
        .map(|(&good, &price)| {
            let stock = stocks.get(&good).copied().unwrap_or(0.0);
            let target = desired_ema.get(&good).copied().unwrap_or(1.0) * BUFFER_TICKS;
            let ratio = if target > 0.0 {
                (stock / target).clamp(0.2, 5.0)
            } else {
                1.0
            };
            // Low stock → ratio < 1 → price / ratio > price → consume less
            // High stock → ratio > 1 → price / ratio < price → consume more
            (good, price / ratio)
        })
        .collect()
}

/// Nonlinear surplus-release controller over stock/target ratio.
///
/// Returns [0,1]:
/// - near 0 when far below target (preserve surplus stock)
/// - near 1 when comfortably above target (consume surplus freely)
fn surplus_release_factor(stock_to_target_ratio: f64) -> f64 {
    let span = (SURPLUS_RELEASE_RATIO_HIGH - SURPLUS_RELEASE_RATIO_LOW).max(0.001);
    let t = ((stock_to_target_ratio - SURPLUS_RELEASE_RATIO_LOW) / span).clamp(0.0, 1.0);
    t.powf(SURPLUS_RELEASE_GAMMA)
}

/// Approximate per-good subsistence floor from need definitions.
///
/// For each subsistence need served by this good, include requirement converted
/// by the good's efficiency: required_good = requirement / efficiency.
fn subsistence_floor_for_good(profile: &GoodProfile, needs: &HashMap<String, Need>) -> f64 {
    profile
        .contributions
        .iter()
        .filter_map(|contrib| {
            if contrib.efficiency <= 0.0 {
                return None;
            }
            let need = needs.get(&contrib.need_id)?;
            match need.utility_curve {
                crate::needs::UtilityCurve::Subsistence { requirement, .. } => {
                    Some(requirement.max(0.0) / contrib.efficiency)
                }
                _ => None,
            }
        })
        .sum()
}

/// Build effective stocks for actual consumption.
///
/// Pops always have access to a baseline floor (subsistence floor or desired tick
/// demand, whichever is higher). Only stock above that baseline is release-gated
/// by stock/target ratio.
fn capped_actual_stocks(
    stocks: &HashMap<GoodId, Quantity>,
    good_profiles: &[GoodProfile],
    needs: &HashMap<String, Need>,
    desired_ema: &HashMap<GoodId, Quantity>,
) -> HashMap<GoodId, Quantity> {
    let mut capped = stocks.clone();

    for profile in good_profiles {
        let good = profile.good;
        let stock = stocks.get(&good).copied().unwrap_or(0.0);
        if stock <= 0.0 {
            continue;
        }

        let desired_tick = desired_ema.get(&good).copied().unwrap_or(0.0).max(0.0);
        let target = desired_tick * BUFFER_TICKS;
        let norm_c = if target > 0.0 {
            (stock / target).clamp(0.0, 10.0)
        } else {
            // Neutral release when no target is known.
            1.0
        };

        let baseline_floor = subsistence_floor_for_good(profile, needs)
            .max(desired_tick)
            .clamp(0.0, stock);

        let cap = if stock <= baseline_floor {
            stock
        } else {
            let surplus = stock - baseline_floor;
            baseline_floor + surplus_release_factor(norm_c) * surplus
        };

        capped.insert(good, cap.clamp(0.0, stock));
    }

    capped
}

// === CONSUMPTION ===

/// Greedy consumption: iteratively pick the good with highest marginal utility per price.
///
/// - `prices`: used to compute MU/price score for ranking
/// - `budget`: if Some, stops when budget exhausted (for desire discovery)
pub fn greedy_consume(
    stocks: &HashMap<GoodId, Quantity>,
    good_profiles: &[GoodProfile],
    needs: &HashMap<String, Need>,
    need_satisfaction: &mut HashMap<String, f64>,
    prices: &HashMap<GoodId, Price>,
    budget: Option<f64>,
) -> HashMap<GoodId, Quantity> {
    let mut remaining_stocks = stocks.clone();
    let mut consumed: HashMap<GoodId, Quantity> = HashMap::new();
    let mut remaining_budget = budget.unwrap_or(f64::INFINITY);

    loop {
        let mut best: Option<(GoodId, f64, f64)> = None; // (good, score, price)

        for profile in good_profiles {
            let available = remaining_stocks.get(&profile.good).copied().unwrap_or(0.0);
            if available <= 0.0 {
                continue;
            }

            let price = prices.get(&profile.good).copied().unwrap_or(1.0).max(0.001);

            // Skip if we can't afford any of this good
            if remaining_budget < price * 0.01 {
                continue;
            }

            // Sum marginal utility across all needs this good serves
            let mut total_mu = 0.0;
            for contrib in &profile.contributions {
                if let Some(need) = needs.get(&contrib.need_id) {
                    let current = need_satisfaction
                        .get(&contrib.need_id)
                        .copied()
                        .unwrap_or(0.0);
                    total_mu += contrib.efficiency * need.utility_curve.marginal_utility(current);
                }
            }

            let score = total_mu / price;

            if score > 0.0 && best.is_none_or(|(_, best_score, _)| score > best_score) {
                best = Some((profile.good, score, price));
            }
        }

        match best {
            Some((good, _, price)) => {
                let available = remaining_stocks.get(&good).copied().unwrap_or(0.0);
                // Consume up to 1 unit, but respect budget and availability
                let max_by_budget = remaining_budget / price;
                let delta = available.min(1.0).min(max_by_budget);

                if delta < 0.001 {
                    break;
                }

                *remaining_stocks.get_mut(&good).unwrap() -= delta;
                *consumed.entry(good).or_insert(0.0) += delta;
                remaining_budget -= delta * price;

                // Update need satisfaction
                let profile = good_profiles.iter().find(|p| p.good == good).unwrap();
                for contrib in &profile.contributions {
                    *need_satisfaction
                        .entry(contrib.need_id.clone())
                        .or_insert(0.0) += contrib.efficiency * delta;
                }
            }
            None => break,
        }
    }

    consumed
}

/// Compute consumption for a population tick.
///
/// - Discovery pass: real stocks, budget = income_ema, market prices → `desired`
///   (What would I buy with my typical income at current prices?)
/// - Actual pass: real stocks, no budget, biased prices → `actual`
///   (Consume from stockpile, conserving when low, indulging when abundant)
pub fn compute_consumption(
    stocks: &HashMap<GoodId, Quantity>,
    good_profiles: &[GoodProfile],
    needs: &HashMap<String, Need>,
    need_satisfaction: &mut HashMap<String, f64>,
    price_ema: &HashMap<GoodId, Price>,
    income_ema: f64,
    desired_ema: &HashMap<GoodId, Quantity>,
) -> ConsumptionResult {
    // Discovery pass: what would I buy with income_ema at current prices?
    let mut discovery_satisfaction = need_satisfaction.clone();
    let desired = greedy_consume(
        stocks,
        good_profiles,
        needs,
        &mut discovery_satisfaction,
        price_ema,
        Some(income_ema),
    );

    // Actual pass: consume from stockpile with bias based on buffer levels
    let biased = biased_prices(stocks, desired_ema, price_ema);
    let capped_stocks = capped_actual_stocks(stocks, good_profiles, needs, desired_ema);
    let actual = greedy_consume(
        &capped_stocks,
        good_profiles,
        needs,
        need_satisfaction,
        &biased,
        None, // no budget for actual consumption
    );

    ConsumptionResult { actual, desired }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NeedContribution;

    const GRAIN: GoodId = 1;

    fn food_profile() -> Vec<GoodProfile> {
        vec![GoodProfile {
            good: GRAIN,
            contributions: vec![NeedContribution {
                need_id: "food".to_string(),
                efficiency: 1.0,
            }],
        }]
    }

    fn food_need() -> HashMap<String, Need> {
        let mut needs = HashMap::new();
        needs.insert(
            "food".to_string(),
            Need {
                id: "food".to_string(),
                utility_curve: crate::needs::UtilityCurve::Subsistence {
                    requirement: 1.0,
                    steepness: 5.0,
                },
            },
        );
        needs
    }

    #[test]
    fn surplus_release_factor_is_monotone() {
        let xs = [0.2, 0.6, 1.0, 1.4, 2.0];
        let ys: Vec<f64> = xs.iter().map(|&x| surplus_release_factor(x)).collect();
        assert!(ys.windows(2).all(|w| w[1] >= w[0]));
        assert!(ys[0] <= 1e-9);
        assert!((ys[ys.len() - 1] - 1.0).abs() <= 1e-9);
    }

    #[test]
    fn capped_actual_stocks_preserves_floor_and_limits_surplus() {
        let mut stocks = HashMap::new();
        stocks.insert(GRAIN, 5.0);
        let mut desired_ema = HashMap::new();
        desired_ema.insert(GRAIN, 1.0);

        let caps = capped_actual_stocks(&stocks, &food_profile(), &food_need(), &desired_ema);
        let cap = caps.get(&GRAIN).copied().unwrap_or(0.0);
        assert!(cap > 1.0, "cap should permit subsistence+ consumption");
        assert!(cap < 5.0, "cap should not allow full depletion near target");
    }

    #[test]
    fn actual_consumption_obeys_stock_cap() {
        let mut stocks = HashMap::new();
        stocks.insert(GRAIN, 5.0);
        let mut desired_ema = HashMap::new();
        desired_ema.insert(GRAIN, 1.0);
        let mut prices = HashMap::new();
        prices.insert(GRAIN, 1.0);

        let profiles = food_profile();
        let needs = food_need();
        let mut sat = HashMap::new();

        let caps = capped_actual_stocks(&stocks, &profiles, &needs, &desired_ema);
        let cap = caps.get(&GRAIN).copied().unwrap_or(0.0);
        let result = compute_consumption(
            &stocks,
            &profiles,
            &needs,
            &mut sat,
            &prices,
            100.0,
            &desired_ema,
        );
        let consumed = result.actual.get(&GRAIN).copied().unwrap_or(0.0);
        assert!(
            consumed >= 0.99,
            "should still consume at least subsistence floor"
        );
        assert!(
            consumed <= cap + 1e-6,
            "actual consumption must respect cap"
        );
        assert!(
            consumed < stocks[&GRAIN],
            "should preserve stock buffer near target"
        );
    }
}
