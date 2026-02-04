use std::collections::{HashMap, HashSet};

use crate::agents::Pop;
use crate::market::{PriceBias, clear_single_market};
use crate::production::Facility;
use crate::types::{FacilityId, Price};

use super::production_fn::ProductionFn;
use super::skills::{SkillDef, SkillId, Worker};

// === LABOR ORDERS ===

#[derive(Debug, Clone)]
pub struct LaborBid {
    pub id: u64,
    pub facility_id: FacilityId,
    pub skill: SkillId,
    pub max_wage: Price, // What we're actually bidding (may be < MVP)
}

/// Labor supply order from a worker.
/// Uses u32 worker_id to support both Worker and Pop as labor sources.
#[derive(Clone)]
pub struct LaborAsk {
    pub id: u64,
    pub worker_id: u32, // Generic ID - can be WorkerId.0 or PopId.0
    pub skill: SkillId,
    pub min_wage: Price,
}

// === MVP OPTIMIZATION ===

/// Given we're hiring `fixed_count` of `fixed_skill`, find optimal quantities
/// of other skills assuming they cost their EMA prices.
/// Returns total cost of optimally-hired other workers.
fn optimize_other_inputs(
    current_workers: &HashMap<SkillId, u32>,
    fixed_skill: SkillId,
    fixed_count: u32,
    wage_emas: &HashMap<SkillId, Price>,
    production_fn: &dyn ProductionFn,
    output_price: Price,
) -> f64 {
    let other_skills: Vec<_> = production_fn
        .relevant_skills()
        .into_iter()
        .filter(|&s| s != fixed_skill)
        .collect();

    if other_skills.is_empty() {
        return 0.0;
    }

    // Start from current allocation with fixed skill set
    let mut best_workers = current_workers.clone();
    best_workers.insert(fixed_skill, fixed_count);

    let mut total_other_cost = compute_other_cost(&best_workers, wage_emas, fixed_skill);

    // Greedy hill-climb: repeatedly add the most profitable marginal worker
    loop {
        let mut best_addition: Option<(SkillId, f64)> = None;

        for &skill in &other_skills {
            let ema = *wage_emas.get(&skill).unwrap_or(&0.0);

            // Try adding one more of this skill
            let mut test_workers = best_workers.clone();
            *test_workers.entry(skill).or_insert(0) += 1;

            let new_output = production_fn.compute(&test_workers);
            let old_output = production_fn.compute(&best_workers);
            let marginal_value = (new_output - old_output) * output_price;
            let marginal_profit = marginal_value - ema;

            if marginal_profit > 0.0 {
                match &best_addition {
                    None => best_addition = Some((skill, marginal_profit)),
                    Some((_, best_mp)) if marginal_profit > *best_mp => {
                        best_addition = Some((skill, marginal_profit));
                    }
                    _ => {}
                }
            }
        }

        match best_addition {
            Some((skill, _)) => {
                let ema = *wage_emas.get(&skill).unwrap_or(&0.0);
                *best_workers.entry(skill).or_insert(0) += 1;
                total_other_cost += ema;
            }
            None => break,
        }
    }

    total_other_cost
}

fn compute_other_cost(
    workers: &HashMap<SkillId, u32>,
    wage_emas: &HashMap<SkillId, Price>,
    exclude_skill: SkillId,
) -> f64 {
    workers
        .iter()
        .filter(|(skill, _)| **skill != exclude_skill)
        .map(|(skill, count)| {
            let ema = *wage_emas.get(skill).unwrap_or(&0.0);
            ema * *count as f64
        })
        .sum()
}

// === BID GENERATION ===

/// Result of generating bids for a facility.
/// Contains both the bids and their corresponding MVPs (for computing profitable_unfilled).
#[derive(Debug, Clone)]
pub struct FacilityBidsResult {
    /// The labor bids (max_wage may be capped by adaptive_bid)
    pub bids: Vec<LaborBid>,
    /// MVP for each bid (in same order as bids), used to compute profitable_unfilled
    pub mvps: Vec<Price>,
}

/// Generate labor bids for a facility based on MVP calculations.
///
/// - `adaptive_bid`: If provided, each slot's max_wage is capped at min(mvp, adaptive_bid).
///   This implements the "bid low but up to MVP" strategy.
///
/// Returns both the bids and their MVPs, so the caller can compute which unfilled
/// slots were "profitable" (MVP > adaptive_bid).
pub fn generate_facility_bids(
    facility: &Facility,
    production_fn: &dyn ProductionFn,
    wage_emas: &HashMap<SkillId, Price>,
    output_price: Price,
    max_hires_per_skill: u32,
    adaptive_bid: Option<Price>,
) -> FacilityBidsResult {
    let mut bids = Vec::new();
    let mut mvps = Vec::new();
    let mut next_id = 0u64;

    for skill in production_fn.relevant_skills() {
        let current_count = facility.workers.get(&skill).copied().unwrap_or(0);

        // Generate bids for each potential marginal hire
        for n in (current_count + 1)..=(current_count + max_hires_per_skill) {
            // Compute optimal complement at EMAs
            let _other_cost = optimize_other_inputs(
                &facility.workers,
                skill,
                n,
                wage_emas,
                production_fn,
                output_price,
            );

            // Output with vs without this marginal worker
            let mut with_worker = facility.workers.clone();
            with_worker.insert(skill, n);

            let mut without_worker = facility.workers.clone();
            without_worker.insert(skill, n - 1);

            // Also set optimal other workers for fair comparison
            let output_with = production_fn.compute(&with_worker);
            let output_without = production_fn.compute(&without_worker);

            let marginal_output = output_with - output_without;
            let mvp = marginal_output * output_price;

            if mvp > 0.0 {
                // Actual bid is min(adaptive_bid, mvp), or just mvp if no adaptive_bid
                let actual_bid = adaptive_bid.map(|ab| ab.min(mvp)).unwrap_or(mvp);

                bids.push(LaborBid {
                    id: next_id,
                    facility_id: facility.id,
                    skill,
                    max_wage: actual_bid,
                });
                mvps.push(mvp);
                next_id += 1;
            }
        }
    }

    FacilityBidsResult { bids, mvps }
}

/// Generate labor asks for a worker across all their skills
pub fn generate_worker_asks(worker: &Worker) -> Vec<LaborAsk> {
    let mut next_id = 0u64;
    worker
        .skills
        .iter()
        .map(|&skill| {
            let ask = LaborAsk {
                id: next_id,
                worker_id: worker.id.0,
                skill,
                min_wage: worker.min_wage,
            };
            next_id += 1;
            ask
        })
        .collect()
}

/// Generate labor asks for a pop across all their skills.
/// Pops participate in labor market as a single worker unit.
pub fn generate_pop_asks(pop: &Pop, next_id: &mut u64) -> Vec<LaborAsk> {
    pop.skills
        .iter()
        .map(|&skill| {
            let ask = LaborAsk {
                id: *next_id,
                worker_id: pop.id.0, // Use pop ID as worker ID
                skill,
                min_wage: pop.min_wage,
            };
            *next_id += 1;
            ask
        })
        .collect()
}

// === MARKET CLEARING ===

/// Assignment of a worker to a facility at a wage.
/// worker_id is generic (u32) to support both Worker and Pop.
pub struct Assignment {
    pub worker_id: u32,
    pub facility_id: FacilityId,
    pub skill: SkillId,
    pub wage: Price,
}

pub struct LaborMarketResult {
    pub clearing_wages: HashMap<SkillId, Price>,
    pub assignments: Vec<Assignment>,
}

/// Convert labor bids/asks to generic Orders for the auction
fn to_orders(bids: &[LaborBid], asks: &[LaborAsk], skill: SkillId) -> Vec<crate::market::Order> {
    use crate::market::{Order, Side};

    let mut orders = Vec::new();

    for bid in bids.iter().filter(|b| b.skill == skill) {
        orders.push(Order {
            id: bid.id,
            agent_id: bid.facility_id.0,
            good: skill.0,
            side: Side::Buy,
            quantity: 1.0,
            limit_price: bid.max_wage,
        });
    }

    for ask in asks.iter().filter(|a| a.skill == skill) {
        orders.push(Order {
            id: ask.id,
            agent_id: ask.worker_id,
            good: skill.0,
            side: Side::Sell,
            quantity: 1.0,
            limit_price: ask.min_wage,
        });
    }

    orders
}

/// Clear labor markets sequentially by wage EMA (highest first)
pub fn clear_labor_markets(
    skills: &[SkillDef],
    bids: &[LaborBid],
    asks: &[LaborAsk],
    wage_emas: &HashMap<SkillId, Price>,
    facility_budgets: &HashMap<FacilityId, f64>,
) -> LaborMarketResult {
    use crate::market::Side;

    // 1. Order skills by wage EMA descending (specialists first)
    let mut skill_order: Vec<_> = skills.iter().map(|s| s.id).collect();
    skill_order.sort_by(|a, b| {
        let ema_a = wage_emas.get(a).unwrap_or(&0.0);
        let ema_b = wage_emas.get(b).unwrap_or(&0.0);
        ema_b.partial_cmp(ema_a).unwrap()
    });

    let mut assignments = Vec::new();
    let mut clearing_wages = HashMap::new();
    let mut filled_workers: HashSet<u32> = HashSet::new();
    let mut remaining_budgets = facility_budgets.clone();

    // Track which bids have been used (one hire per bid)
    let mut used_bids: HashSet<u64> = HashSet::new();

    // 2. Clear each skill market in order
    for skill in skill_order {
        // Track bids removed due to budget constraints (for this skill market only)
        let mut removed_bids: HashSet<u64> = HashSet::new();

        // Iterative clearing with budget relaxation
        loop {
            // Filter bids: this skill, unused, not removed
            let skill_bids: Vec<_> = bids
                .iter()
                .filter(|b| b.skill == skill)
                .filter(|b| !used_bids.contains(&b.id))
                .filter(|b| !removed_bids.contains(&b.id))
                .collect();

            // Filter asks: this skill, worker not already hired
            let skill_asks: Vec<_> = asks
                .iter()
                .filter(|a| a.skill == skill)
                .filter(|a| !filled_workers.contains(&a.worker_id))
                .collect();

            if skill_bids.is_empty() || skill_asks.is_empty() {
                break;
            }

            // Convert to Order format
            let orders = to_orders(
                &skill_bids.iter().map(|b| (*b).clone()).collect::<Vec<_>>(),
                &skill_asks.iter().map(|a| (*a).clone()).collect::<Vec<_>>(),
                skill,
            );

            // Clear with worker (seller) bias - wages set by employer's bid
            // This means clearing wage = min(filled bids), so the marginal
            // employer sets the wage. Facilities adjust bids based on fill rate.
            let result = clear_single_market(skill.0, &orders, PriceBias::FavorSellers);

            let Some(wage) = result.clearing_price else {
                break;
            };

            // Check which facilities can't afford fills at clearing price
            let buy_fills: Vec<_> = result
                .fills
                .iter()
                .filter(|f| matches!(f.side, Side::Buy))
                .collect();

            // Calculate cumulative cost per facility from all their fills
            let mut facility_costs: HashMap<FacilityId, f64> = HashMap::new();
            for buy_fill in &buy_fills {
                let bid = skill_bids
                    .iter()
                    .find(|b| b.id == buy_fill.order_id)
                    .unwrap();
                *facility_costs.entry(bid.facility_id).or_insert(0.0) += wage;
            }

            // Find bids from facilities that exceed budget
            let mut infeasible_bids = Vec::new();
            for buy_fill in &buy_fills {
                let bid = skill_bids
                    .iter()
                    .find(|b| b.id == buy_fill.order_id)
                    .unwrap();

                let budget = remaining_budgets
                    .get(&bid.facility_id)
                    .copied()
                    .unwrap_or(0.0);
                let total_cost = *facility_costs.get(&bid.facility_id).unwrap();

                // If total cost exceeds budget, mark the lowest-priority bid as infeasible
                // (we process in order, so later bids get removed first)
                if total_cost > budget {
                    infeasible_bids.push(bid.id);
                    // Reduce tracked cost so we keep one bid if budget allows at least one
                    *facility_costs.get_mut(&bid.facility_id).unwrap() -= wage;
                }
            }

            // If any infeasible, remove them and re-clear
            if !infeasible_bids.is_empty() {
                for bid_id in infeasible_bids {
                    removed_bids.insert(bid_id);
                }
                continue;
            }

            // All feasible - commit the fills
            clearing_wages.insert(skill, wage);

            let sell_fills: Vec<_> = result
                .fills
                .iter()
                .filter(|f| matches!(f.side, Side::Sell))
                .collect();

            // Pair them up (assumes 1:1 quantity fills)
            for (buy_fill, sell_fill) in buy_fills.iter().zip(sell_fills.iter()) {
                let bid = skill_bids
                    .iter()
                    .find(|b| b.id == buy_fill.order_id)
                    .unwrap();
                let ask = skill_asks
                    .iter()
                    .find(|a| a.id == sell_fill.order_id)
                    .unwrap();

                // Deduct from facility budget
                if let Some(budget) = remaining_budgets.get_mut(&bid.facility_id) {
                    *budget -= wage;
                }

                // Mark bid as used
                used_bids.insert(bid.id);

                // Mark worker as filled
                filled_workers.insert(ask.worker_id);

                // Record assignment
                assignments.push(Assignment {
                    worker_id: ask.worker_id,
                    facility_id: bid.facility_id,
                    skill,
                    wage,
                });
            }

            break;
        }
    }

    LaborMarketResult {
        clearing_wages,
        assignments,
    }
}

/// Update wage EMAs after market clearing
pub fn update_wage_emas(wage_emas: &mut HashMap<SkillId, Price>, result: &LaborMarketResult) {
    for (skill, wage) in &result.clearing_wages {
        let ema = wage_emas.entry(*skill).or_insert(*wage);
        *ema = 0.7 * *ema + 0.3 * wage;
    }
}

/// Apply labor assignments to facilities
pub fn apply_assignments(facilities: &mut [Facility], result: &LaborMarketResult) {
    for assignment in &result.assignments {
        if let Some(facility) = facilities
            .iter_mut()
            .find(|f| f.id == assignment.facility_id)
        {
            facility.currency -= assignment.wage;
            *facility.workers.entry(assignment.skill).or_insert(0) += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::skills::WorkerId;
    use super::*;

    // === TEST HELPERS ===

    fn skill(id: u32) -> SkillId {
        SkillId(id)
    }

    fn skill_def(id: u32, name: &str, parent: Option<u32>) -> SkillDef {
        SkillDef {
            id: SkillId(id),
            name: name.to_string(),
            parent: parent.map(SkillId),
        }
    }

    fn worker(id: u32, skills: &[u32], min_wage: f64) -> Worker {
        Worker {
            id: WorkerId(id),
            skills: skills.iter().map(|&s| SkillId(s)).collect(),
            min_wage,
        }
    }

    fn bid(id: u64, facility: u32, skill: u32, max_wage: f64) -> LaborBid {
        LaborBid {
            id,
            facility_id: FacilityId(facility),
            skill: SkillId(skill),
            max_wage,
        }
    }

    fn ask(id: u64, worker: u32, skill: u32, min_wage: f64) -> LaborAsk {
        LaborAsk {
            id,
            worker_id: worker,
            skill: SkillId(skill),
            min_wage,
        }
    }

    fn budgets(pairs: &[(u32, f64)]) -> HashMap<FacilityId, f64> {
        pairs
            .iter()
            .map(|&(id, amt)| (FacilityId(id), amt))
            .collect()
    }

    fn emas(pairs: &[(u32, f64)]) -> HashMap<SkillId, f64> {
        pairs.iter().map(|&(id, amt)| (SkillId(id), amt)).collect()
    }

    // === TESTS ===

    #[test]
    fn clearing_wage_at_employer_bid() {
        // Facility: will pay up to 5
        // Worker: will work for 0+
        // With FavorSellers, clearing = employer's bid (5)
        let skills = vec![skill_def(1, "Laborer", None)];

        let bids = vec![bid(1, 1, 1, 5.0)];
        let asks = vec![ask(1, 1, 1, 0.0)];

        let wage_emas = emas(&[(1, 2.0)]);
        let facility_budgets = budgets(&[(1, 100.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        assert_eq!(result.assignments.len(), 1);
        let wage = *result.clearing_wages.get(&skill(1)).unwrap();
        assert_eq!(
            wage, 5.0,
            "clearing wage should be employer's bid (5), got {}",
            wage
        );
    }

    #[test]
    fn no_double_hire() {
        // Worker has both skills, two facilities each want one
        let skills = vec![
            skill_def(1, "Laborer", None),
            skill_def(2, "Craftsman", Some(1)),
        ];

        let _workers = [worker(1, &[1, 2], 0.0)];

        // Facility A bids for craftsman, Facility B bids for laborer
        let bids = vec![
            bid(1, 1, 2, 50.0), // Facility 1 wants craftsman @ 50
            bid(2, 2, 1, 30.0), // Facility 2 wants laborer @ 30
        ];

        // Worker asks in both markets
        let asks = vec![
            ask(1, 1, 2, 0.0), // Worker 1 as craftsman
            ask(2, 1, 1, 0.0), // Worker 1 as laborer
        ];

        let wage_emas = emas(&[(1, 25.0), (2, 40.0)]); // craftsman EMA higher
        let facility_budgets = budgets(&[(1, 100.0), (2, 100.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        // Worker should only be assigned once
        assert_eq!(result.assignments.len(), 1);

        // Should be assigned as craftsman (higher EMA clears first)
        assert_eq!(result.assignments[0].skill, skill(2));
    }

    #[test]
    fn no_trade_when_bid_below_ask() {
        let skills = vec![skill_def(1, "Laborer", None)];

        // Facility bids 10, but worker wants at least 20
        let bids = vec![bid(1, 1, 1, 10.0)];
        let asks = vec![ask(1, 1, 1, 20.0)];

        let wage_emas = emas(&[(1, 15.0)]);
        let facility_budgets = budgets(&[(1, 100.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        assert!(result.assignments.is_empty());
        assert!(result.clearing_wages.is_empty());
    }

    #[test]
    fn budget_respected() {
        let skills = vec![skill_def(1, "Laborer", None)];

        // Two workers, facility can only afford one
        let bids = vec![bid(1, 1, 1, 50.0), bid(2, 1, 1, 50.0)];
        let asks = vec![ask(1, 1, 1, 0.0), ask(2, 2, 1, 0.0)];

        let wage_emas = emas(&[(1, 40.0)]);
        let facility_budgets = budgets(&[(1, 50.0)]); // can only afford one @ clearing price

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        // Total wages paid should not exceed budget
        let total_paid: f64 = result
            .assignments
            .iter()
            .filter(|a| a.facility_id == FacilityId(1))
            .map(|a| a.wage)
            .sum();

        assert!(
            total_paid <= 50.0,
            "total_paid {} exceeds budget 50",
            total_paid
        );
    }

    #[test]
    fn wage_within_bounds() {
        let skills = vec![skill_def(1, "Laborer", None)];

        // Bids at various prices, asks at various prices
        let bids = vec![
            bid(1, 1, 1, 100.0), // willing to pay up to 100
            bid(2, 2, 1, 60.0),  // willing to pay up to 60
            bid(3, 3, 1, 30.0),  // willing to pay up to 30
        ];
        let asks = vec![
            ask(1, 1, 1, 10.0), // wants at least 10
            ask(2, 2, 1, 40.0), // wants at least 40
            ask(3, 3, 1, 80.0), // wants at least 80
        ];

        let wage_emas = emas(&[(1, 50.0)]);
        let facility_budgets = budgets(&[(1, 200.0), (2, 200.0), (3, 200.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        // Should have some trades
        assert!(!result.assignments.is_empty());

        if let Some(&wage) = result.clearing_wages.get(&skill(1)) {
            // Wage should be >= lowest filled ask
            let min_filled_ask = result
                .assignments
                .iter()
                .filter_map(|a| asks.iter().find(|ask| ask.worker_id == a.worker_id))
                .map(|a| a.min_wage)
                .fold(f64::INFINITY, f64::min);

            // Wage should be <= highest filled bid
            let max_filled_bid = result
                .assignments
                .iter()
                .filter_map(|a| bids.iter().find(|b| b.facility_id == a.facility_id))
                .map(|b| b.max_wage)
                .fold(f64::NEG_INFINITY, f64::max);

            assert!(
                wage >= min_filled_ask,
                "wage {} < min ask {}",
                wage,
                min_filled_ask
            );
            assert!(
                wage <= max_filled_bid,
                "wage {} > max bid {}",
                wage,
                max_filled_bid
            );
        }
    }

    #[test]
    fn seller_bias_picks_higher_price() {
        let skills = vec![skill_def(1, "Laborer", None)];

        // One bid at 100, one ask at 0
        // Valid clearing prices are anywhere in [0, 100]
        // With FavorSellers, should pick high end (employer's bid)
        let bids = vec![bid(1, 1, 1, 100.0)];
        let asks = vec![ask(1, 1, 1, 0.0)];

        let wage_emas = emas(&[(1, 50.0)]);
        let facility_budgets = budgets(&[(1, 200.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        assert_eq!(result.assignments.len(), 1);

        // With seller bias, wage should be at the bid (100), not the ask (0)
        let wage = result.clearing_wages.get(&skill(1)).unwrap();
        assert_eq!(*wage, 100.0, "expected wage=100 (bid), got {}", wage);
    }

    #[test]
    fn specialist_assigned_to_specialty_first() {
        // Smith (high EMA) should clear before Laborer (low EMA)
        // So a worker who is both should get the smith job
        let skills = vec![
            skill_def(1, "Laborer", None),
            skill_def(2, "Smith", Some(1)),
        ];

        // Two facilities: one wants smith, one wants laborer
        // Only one worker who can do both
        let bids = vec![
            bid(1, 1, 2, 80.0), // Facility 1 wants smith
            bid(2, 2, 1, 80.0), // Facility 2 wants laborer
        ];
        let asks = vec![
            ask(1, 1, 2, 0.0), // Worker as smith
            ask(2, 1, 1, 0.0), // Worker as laborer
        ];

        // Smith has higher EMA, so clears first
        let wage_emas = emas(&[(1, 20.0), (2, 60.0)]);
        let facility_budgets = budgets(&[(1, 200.0), (2, 200.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        assert_eq!(result.assignments.len(), 1);
        assert_eq!(result.assignments[0].skill, skill(2)); // assigned as smith
        assert_eq!(result.assignments[0].facility_id, FacilityId(1));
    }

    #[test]
    fn complementarity_increases_output() {
        use super::super::production_fn::ComplementaryProductionFn;

        let laborer = SkillId(1);
        let craftsman = SkillId(2);

        let prod_fn = ComplementaryProductionFn {
            base_output: [(laborer, 1.0), (craftsman, 5.0)].into(),
            complementarity_bonus: [((laborer, craftsman), 2.0)].into(), // +2 per pair
            max_optimal_capacity: [(laborer, 10), (craftsman, 10)].into(),
            diminishing_rate: 0.1,
        };

        let just_laborer: HashMap<SkillId, u32> = [(laborer, 1)].into();
        let just_craftsman: HashMap<SkillId, u32> = [(craftsman, 1)].into();
        let both: HashMap<SkillId, u32> = [(laborer, 1), (craftsman, 1)].into();

        use super::super::production_fn::ProductionFn;
        let output_laborer = prod_fn.compute(&just_laborer);
        let output_craftsman = prod_fn.compute(&just_craftsman);
        let output_both = prod_fn.compute(&both);

        assert_eq!(output_laborer, 1.0);
        assert_eq!(output_craftsman, 5.0);
        assert_eq!(output_both, 8.0); // 1 + 5 + 2 bonus

        // Complementarity: together > sum of parts
        assert!(output_both > output_laborer + output_craftsman);
    }

    #[test]
    fn facility_cant_hire_when_bid_exceeds_budget() {
        let skills = vec![skill_def(1, "Laborer", None)];

        // Facility bids 50, but only has budget of 40
        // Worker asks 10
        // With FavorSellers, clearing = 50 (bid), which exceeds budget
        let bids = vec![bid(1, 1, 1, 50.0)];
        let asks = vec![ask(1, 1, 1, 10.0)];

        let wage_emas = emas(&[(1, 30.0)]);
        let facility_budgets = budgets(&[(1, 40.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        // Should NOT hire because clearing price (50) exceeds budget (40)
        assert_eq!(result.assignments.len(), 0);
    }

    #[test]
    fn facility_can_hire_when_bid_within_budget() {
        let skills = vec![skill_def(1, "Laborer", None)];

        // Facility bids 30, has budget of 40
        // Worker asks 10
        // With FavorSellers, clearing = 30 (bid), which is within budget
        let bids = vec![bid(1, 1, 1, 30.0)];
        let asks = vec![ask(1, 1, 1, 10.0)];

        let wage_emas = emas(&[(1, 20.0)]);
        let facility_budgets = budgets(&[(1, 40.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        // Should hire at 30 (bid), which is within budget
        assert_eq!(result.assignments.len(), 1);
        assert_eq!(*result.clearing_wages.get(&skill(1)).unwrap(), 30.0);
    }

    #[test]
    fn excess_workers_wage_at_employer_bid() {
        let skills = vec![skill_def(1, "Laborer", None)];

        // 1 job, 5 workers all willing to work for 0
        // With FavorSellers, wage = employer's bid (50)
        // The "floor" effect happens over time as employers lower bids
        let bids = vec![bid(1, 1, 1, 50.0)];
        let asks = vec![
            ask(1, 1, 1, 0.0),
            ask(2, 2, 1, 0.0),
            ask(3, 3, 1, 0.0),
            ask(4, 4, 1, 0.0),
            ask(5, 5, 1, 0.0),
        ];

        let wage_emas = emas(&[(1, 25.0)]);
        let facility_budgets = budgets(&[(1, 100.0)]);

        let result = clear_labor_markets(&skills, &bids, &asks, &wage_emas, &facility_budgets);

        assert_eq!(result.assignments.len(), 1);

        // With FavorSellers, wage = employer's bid
        let wage = *result.clearing_wages.get(&skill(1)).unwrap();
        assert_eq!(wage, 50.0, "wage should equal employer's bid");
    }

    #[test]
    fn diminishing_returns_past_capacity() {
        use super::super::production_fn::ComplementaryProductionFn;

        let laborer = SkillId(1);

        let prod_fn = ComplementaryProductionFn {
            base_output: [(laborer, 10.0)].into(),
            complementarity_bonus: HashMap::new(),
            max_optimal_capacity: [(laborer, 2)].into(), // only 2 at full productivity
            diminishing_rate: 0.5,                       // lose 50% per worker over capacity
        };

        let one: HashMap<SkillId, u32> = [(laborer, 1)].into();
        let two: HashMap<SkillId, u32> = [(laborer, 2)].into();
        let three: HashMap<SkillId, u32> = [(laborer, 3)].into();
        let four: HashMap<SkillId, u32> = [(laborer, 4)].into();

        use super::super::production_fn::ProductionFn;
        let out_1 = prod_fn.compute(&one);
        let out_2 = prod_fn.compute(&two);
        let out_3 = prod_fn.compute(&three);
        let out_4 = prod_fn.compute(&four);

        // First two at full productivity
        assert_eq!(out_1, 10.0);
        assert_eq!(out_2, 20.0);

        // Third worker: 50% productivity (1 over capacity * 0.5 rate)
        assert_eq!(out_3, 25.0); // 20 + 10*0.5

        // Fourth worker: 0% productivity (2 over capacity * 0.5 rate = 1.0, clamped to 0)
        assert_eq!(out_4, 25.0); // no additional output

        // Marginal product decreases
        let mp_1 = out_1;
        let mp_2 = out_2 - out_1;
        let mp_3 = out_3 - out_2;
        let mp_4 = out_4 - out_3;

        assert_eq!(mp_1, 10.0);
        assert_eq!(mp_2, 10.0);
        assert_eq!(mp_3, 5.0);
        assert_eq!(mp_4, 0.0);
    }

    /// Test that wages rise when labor becomes scarce.
    ///
    /// Scenario:
    /// - 2 facilities, each wants 3 workers (6 total slots)
    /// - Start with 6 workers (no scarcity) → wages low
    /// - Remove 2 workers (now 4 workers, 6 slots) → scarcity
    /// - Facilities compete, bidding up to their MVP
    /// - Wages rise until one facility is priced out
    #[test]
    fn scarcity_drives_wages_up() {
        let skills = vec![skill_def(1, "Laborer", None)];
        let laborer = skill(1);

        // Two facilities with different MVPs
        // Facility 1: MVP = 100 (high value production)
        // Facility 2: MVP = 50 (lower value production)
        // Each wants 3 workers
        let facility_1_mvp = 100.0;
        let facility_2_mvp = 50.0;

        // Workers all have min_wage = 10
        let worker_min_wage = 10.0;

        // --- Phase 1: No scarcity (6 workers, 6 slots) ---
        let bids_full = vec![
            // Facility 1 bids for 3 workers at MVP=100
            bid(1, 1, 1, facility_1_mvp),
            bid(2, 1, 1, facility_1_mvp),
            bid(3, 1, 1, facility_1_mvp),
            // Facility 2 bids for 3 workers at MVP=50
            bid(4, 2, 1, facility_2_mvp),
            bid(5, 2, 1, facility_2_mvp),
            bid(6, 2, 1, facility_2_mvp),
        ];

        let asks_full = vec![
            ask(1, 1, 1, worker_min_wage),
            ask(2, 2, 1, worker_min_wage),
            ask(3, 3, 1, worker_min_wage),
            ask(4, 4, 1, worker_min_wage),
            ask(5, 5, 1, worker_min_wage),
            ask(6, 6, 1, worker_min_wage),
        ];

        let mut wage_emas = emas(&[(1, 30.0)]); // starting EMA
        let facility_budgets = budgets(&[(1, 1000.0), (2, 1000.0)]);

        let result_full = clear_labor_markets(
            &skills,
            &bids_full,
            &asks_full,
            &wage_emas,
            &facility_budgets,
        );

        // All 6 workers should be hired
        assert_eq!(
            result_full.assignments.len(),
            6,
            "all workers hired when no scarcity"
        );

        // With FavorSellers, wage = min(filled bids) = 50 (facility 2's bid)
        let wage_no_scarcity = *result_full.clearing_wages.get(&laborer).unwrap();
        assert_eq!(
            wage_no_scarcity, facility_2_mvp,
            "wage at marginal employer's bid (50)"
        );

        // Count workers per facility
        let f1_hires: usize = result_full
            .assignments
            .iter()
            .filter(|a| a.facility_id == FacilityId(1))
            .count();
        let f2_hires: usize = result_full
            .assignments
            .iter()
            .filter(|a| a.facility_id == FacilityId(2))
            .count();
        assert_eq!(f1_hires, 3, "facility 1 gets 3 workers");
        assert_eq!(f2_hires, 3, "facility 2 gets 3 workers");

        // Update wage EMA
        update_wage_emas(&mut wage_emas, &result_full);

        // --- Phase 2: Create scarcity (4 workers, 6 slots) ---
        // Remove 2 workers - now facilities must compete
        let asks_scarce = vec![
            ask(1, 1, 1, worker_min_wage),
            ask(2, 2, 1, worker_min_wage),
            ask(3, 3, 1, worker_min_wage),
            ask(4, 4, 1, worker_min_wage),
            // Workers 5 and 6 are gone
        ];

        let result_scarce = clear_labor_markets(
            &skills,
            &bids_full,
            &asks_scarce,
            &wage_emas,
            &facility_budgets,
        );

        // Only 4 workers can be hired
        assert_eq!(
            result_scarce.assignments.len(),
            4,
            "only 4 workers available"
        );

        // With FavorSellers and 4 workers for 6 slots:
        // Top 4 bids: 100, 100, 100, 50 (3 from facility 1, 1 from facility 2)
        // Clearing = min(filled bids) = 50
        let wage_scarce = *result_scarce.clearing_wages.get(&laborer).unwrap();
        assert_eq!(wage_scarce, 50.0, "wage = min(filled bids) = 50");

        // Count workers per facility - higher MVP facility should get priority
        let f1_hires_scarce: usize = result_scarce
            .assignments
            .iter()
            .filter(|a| a.facility_id == FacilityId(1))
            .count();
        let f2_hires_scarce: usize = result_scarce
            .assignments
            .iter()
            .filter(|a| a.facility_id == FacilityId(2))
            .count();

        // Facility 1 (MVP=100) should get all 3 slots filled
        // Facility 2 (MVP=50) gets remaining 1 worker
        assert_eq!(f1_hires_scarce, 3, "high-MVP facility 1 fills all slots");
        assert_eq!(
            f2_hires_scarce, 1,
            "low-MVP facility 2 gets remaining worker"
        );
    }

    /// Test that when workers have higher min_wage, scarcity DOES raise clearing wage.
    /// Test that wages rise to marginal MVP when there's labor scarcity.
    ///
    /// With 3 workers and 4 slots (MVPs: 100, 80, 60, 40):
    /// - Facility 4 (MVP=40) should be priced out
    /// - Wage should stabilize around 40-60 (the marginal viable MVP)
    #[test]
    fn scarcity_raises_wage_to_marginal_mvp() {
        let skills = vec![skill_def(1, "Laborer", None)];
        let laborer = skill(1);

        // 4 facilities with different MVPs
        let mvps = [100.0, 80.0, 60.0, 40.0];

        // 3 workers (scarcity), all with min_wage = 5
        let asks = vec![ask(1, 1, 1, 5.0), ask(2, 2, 1, 5.0), ask(3, 3, 1, 5.0)];

        // Track each facility's current bid (starts at wage_ema)
        let mut facility_bids = [10.0, 10.0, 10.0, 10.0]; // start low
        let mut wage_ema = emas(&[(1, 10.0)]);
        let facility_budgets = budgets(&[(1, 1000.0), (2, 1000.0), (3, 1000.0), (4, 1000.0)]);

        println!("=== Scarcity Raises Wage to Marginal MVP ===\n");
        println!("Setup: 4 facilities (MVP=100,80,60,40), 3 workers (min_wage=5)");
        println!("Expected: wage rises until facility 4 (MVP=40) is priced out\n");

        let mut last_hires = [false; 4];

        for tick in 0..50 {
            // Build bids based on current facility_bids
            let bids: Vec<_> = (0..4)
                .map(|i| bid((i + 1) as u64, (i + 1) as u32, 1, facility_bids[i]))
                .collect();

            let result = clear_labor_markets(&skills, &bids, &asks, &wage_ema, &facility_budgets);
            let wage = result.clearing_wages.get(&laborer).copied().unwrap_or(0.0);

            // Track which facilities got workers
            let hires: Vec<bool> = (1..=4)
                .map(|f| {
                    result
                        .assignments
                        .iter()
                        .any(|a| a.facility_id == FacilityId(f))
                })
                .collect();

            // Count global workers vs jobs
            let total_workers = asks.len();
            let total_jobs = bids.len();

            if !(5..45).contains(&tick) {
                println!(
                    "Tick {:>2}: bids={:>5.1},{:>5.1},{:>5.1},{:>5.1} | clearing={:>5.1} | hired={:?}",
                    tick,
                    facility_bids[0],
                    facility_bids[1],
                    facility_bids[2],
                    facility_bids[3],
                    wage,
                    hires
                );
            } else if tick == 5 {
                println!("...");
            }

            update_wage_emas(&mut wage_ema, &result);

            // Adjust bids for next tick
            for i in 0..4 {
                let filled = hires[i];
                let mvp = mvps[i];

                if !filled {
                    // Unfilled: raise bid (up to MVP)
                    facility_bids[i] = (facility_bids[i] * 1.2).min(mvp);
                } else if total_workers > total_jobs {
                    // Filled + excess workers: lower bid
                    facility_bids[i] = (facility_bids[i] * 0.95).max(5.0);
                }
                // else: filled + tight market: hold steady
            }

            last_hires = [hires[0], hires[1], hires[2], hires[3]];
        }

        let final_ema = *wage_ema.get(&laborer).unwrap();
        println!("\nFinal state:");
        println!("  Wage EMA: {:.1}", final_ema);
        println!("  Facility 4 (MVP=40) hired: {}", last_hires[3]);

        // Assertions:
        // 1. Wage should have risen significantly above starting point (10)
        assert!(
            final_ema > 30.0,
            "Wage should rise toward marginal MVP (~40), got {:.1}",
            final_ema
        );

        // 2. Facility 4 should eventually be priced out (not hiring)
        assert!(
            !last_hires[3],
            "Facility 4 (MVP=40) should be priced out when wage >= 40"
        );

        // 3. Top 3 facilities should still be hiring
        assert!(
            last_hires[0] && last_hires[1] && last_hires[2],
            "Top 3 facilities should still hire: {:?}",
            last_hires
        );
    }
}
