// Production execution logic

use std::collections::HashMap;

use crate::agents::Stockpile;
use crate::labor::SkillId;
use crate::types::{FacilityId, GoodId, Quantity};

use super::facility::Facility;
use super::recipe::{Recipe, RecipeId};

// === RECIPE ALLOCATION ===

/// Result of allocating recipes to a facility
#[derive(Debug, Clone)]
pub struct RecipeAllocation {
    pub facility_id: FacilityId,
    /// Recipe ID -> number of instances to run
    pub runs: HashMap<RecipeId, u32>,
}

impl RecipeAllocation {
    pub fn new(facility_id: FacilityId) -> Self {
        Self {
            facility_id,
            runs: HashMap::new(),
        }
    }

    pub fn total_runs(&self) -> u32 {
        self.runs.values().sum()
    }
}

/// Allocate recipes to a single facility using greedy priority-based algorithm.
///
/// Hard constraints (all must be satisfied to run an instance):
/// 1. Capacity: facility.capacity >= recipe.capacity_cost
/// 2. Workers: facility.workers[skill] >= recipe.workers[skill] for all skills
/// 3. Inputs: stockpile[good] >= recipe.inputs[good] for all inputs
/// 4. Facility match: recipe.facility_types.contains(facility.facility_type)
///
/// Greedy fills by priority order (first recipe in list = highest priority).
pub fn allocate_recipes(
    facility: &Facility,
    recipes: &[Recipe],
    stockpile: &Stockpile,
) -> RecipeAllocation {
    let mut allocation = RecipeAllocation::new(facility.id);

    // Track remaining resources
    let mut remaining_capacity = facility.capacity;
    let mut remaining_workers: HashMap<SkillId, u32> = facility.workers.clone();
    let mut remaining_inputs: HashMap<GoodId, Quantity> = stockpile.goods.clone();

    // Process recipes in priority order
    for recipe_id in &facility.recipe_priorities {
        // Find the recipe definition
        let Some(recipe) = recipes.iter().find(|r| r.id == *recipe_id) else {
            continue;
        };

        // Check facility type match (constraint 4)
        if !recipe.can_run_at(facility.facility_type) {
            continue;
        }

        // Run as many instances as possible
        let mut instances = 0u32;

        loop {
            // Check capacity (constraint 1)
            if remaining_capacity < recipe.capacity_cost {
                break;
            }

            // Check workers (constraint 2)
            let has_workers = recipe.workers.iter().all(|(skill, needed)| {
                remaining_workers.get(skill).copied().unwrap_or(0) >= *needed
            });
            if !has_workers {
                break;
            }

            // Check inputs (constraint 3)
            let has_inputs = recipe.inputs.iter().all(|(good, needed)| {
                remaining_inputs.get(good).copied().unwrap_or(0.0) >= *needed
            });
            if !has_inputs {
                break;
            }

            // All constraints satisfied - commit to running this instance
            remaining_capacity -= recipe.capacity_cost;

            for (skill, needed) in &recipe.workers {
                if let Some(count) = remaining_workers.get_mut(skill) {
                    *count -= needed;
                }
            }

            for (good, needed) in &recipe.inputs {
                if let Some(qty) = remaining_inputs.get_mut(good) {
                    *qty -= needed;
                }
            }

            instances += 1;
        }

        if instances > 0 {
            allocation.runs.insert(*recipe_id, instances);
        }
    }

    allocation
}

// === PRODUCTION EXECUTION ===

/// Result of running production at a facility
#[derive(Debug, Clone)]
pub struct ProductionResult {
    pub facility_id: FacilityId,
    /// Inputs consumed (good -> quantity)
    pub inputs_consumed: HashMap<GoodId, Quantity>,
    /// Outputs produced (good -> quantity)
    pub outputs_produced: HashMap<GoodId, Quantity>,
    /// Total wages paid
    pub wages_paid: f64,
}

impl ProductionResult {
    pub fn new(facility_id: FacilityId) -> Self {
        Self {
            facility_id,
            inputs_consumed: HashMap::new(),
            outputs_produced: HashMap::new(),
            wages_paid: 0.0,
        }
    }
}

/// Execute production for a facility given its recipe allocation.
///
/// This consumes inputs from the stockpile, produces outputs to the stockpile,
/// and optionally applies a quality multiplier from the resource slot.
pub fn execute_production(
    allocation: &RecipeAllocation,
    recipes: &[Recipe],
    stockpile: &mut Stockpile,
    quality_multiplier: f64,
) -> ProductionResult {
    let mut result = ProductionResult::new(allocation.facility_id);

    for (recipe_id, &instances) in &allocation.runs {
        let Some(recipe) = recipes.iter().find(|r| r.id == *recipe_id) else {
            continue;
        };

        // Consume inputs
        for (good, qty_per_instance) in &recipe.inputs {
            let total_consumed = qty_per_instance * instances as f64;
            stockpile.remove(*good, total_consumed);
            *result.inputs_consumed.entry(*good).or_insert(0.0) += total_consumed;
        }

        // Produce outputs (with quality multiplier)
        for (good, qty_per_instance) in &recipe.outputs {
            let total_produced = qty_per_instance * instances as f64 * quality_multiplier;
            stockpile.add(*good, total_produced);
            *result.outputs_produced.entry(*good).or_insert(0.0) += total_produced;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labor::SkillId;
    use crate::production::{FacilityType, RecipeId};
    use crate::types::{FacilityId, MerchantId, SettlementId};

    // Test goods
    const GRAIN: GoodId = 1;
    const BREAD: GoodId = 2;

    // Test skills
    fn laborer() -> SkillId {
        SkillId(1)
    }
    fn baker() -> SkillId {
        SkillId(2)
    }

    fn make_facility(workers: HashMap<SkillId, u32>, capacity: u32) -> Facility {
        let mut facility = Facility::new(
            FacilityId::new(1),
            FacilityType::Bakery,
            SettlementId::new(1),
            MerchantId::new(1),
        );
        facility.workers = workers;
        facility.capacity = capacity;
        facility
    }

    fn basic_bread_recipe() -> Recipe {
        Recipe::new(RecipeId::new(1), "Basic Bread", vec![FacilityType::Bakery])
            .with_capacity_cost(2)
            .with_worker(baker(), 1)
            .with_input(GRAIN, 2.0)
            .with_output(BREAD, 3.0)
    }

    fn hardtack_recipe() -> Recipe {
        Recipe::new(RecipeId::new(2), "Hardtack", vec![FacilityType::Bakery])
            .with_capacity_cost(1)
            .with_worker(laborer(), 1)
            .with_input(GRAIN, 1.0)
            .with_output(BREAD, 1.5)
    }

    #[test]
    fn test_allocate_single_recipe() {
        let workers: HashMap<SkillId, u32> = [(baker(), 2)].into();
        let mut facility = make_facility(workers, 10);
        facility.recipe_priorities = vec![RecipeId::new(1)];

        let mut stockpile = Stockpile::new();
        stockpile.add(GRAIN, 10.0);

        let recipes = vec![basic_bread_recipe()];
        let allocation = allocate_recipes(&facility, &recipes, &stockpile);

        // With 2 bakers, 10 capacity, 10 grain:
        // - Each recipe needs 1 baker, 2 capacity, 2 grain
        // - Limited by bakers: can run 2 instances
        assert_eq!(allocation.runs.get(&RecipeId::new(1)), Some(&2));
    }

    #[test]
    fn test_allocate_limited_by_capacity() {
        let workers: HashMap<SkillId, u32> = [(baker(), 5)].into();
        let mut facility = make_facility(workers, 4); // Only 4 capacity
        facility.recipe_priorities = vec![RecipeId::new(1)];

        let mut stockpile = Stockpile::new();
        stockpile.add(GRAIN, 20.0);

        let recipes = vec![basic_bread_recipe()]; // costs 2 capacity each
        let allocation = allocate_recipes(&facility, &recipes, &stockpile);

        // Limited by capacity: 4 / 2 = 2 instances
        assert_eq!(allocation.runs.get(&RecipeId::new(1)), Some(&2));
    }

    #[test]
    fn test_allocate_limited_by_inputs() {
        let workers: HashMap<SkillId, u32> = [(baker(), 5)].into();
        let mut facility = make_facility(workers, 20);
        facility.recipe_priorities = vec![RecipeId::new(1)];

        let mut stockpile = Stockpile::new();
        stockpile.add(GRAIN, 5.0); // Only 5 grain

        let recipes = vec![basic_bread_recipe()]; // needs 2 grain each
        let allocation = allocate_recipes(&facility, &recipes, &stockpile);

        // Limited by grain: floor(5 / 2) = 2 instances
        assert_eq!(allocation.runs.get(&RecipeId::new(1)), Some(&2));
    }

    #[test]
    fn test_allocate_priority_order() {
        // Facility has bakers and laborers
        let workers: HashMap<SkillId, u32> = [(baker(), 1), (laborer(), 2)].into();
        let mut facility = make_facility(workers, 10);
        // Prioritize basic bread over hardtack
        facility.recipe_priorities = vec![RecipeId::new(1), RecipeId::new(2)];

        let mut stockpile = Stockpile::new();
        stockpile.add(GRAIN, 10.0);

        let recipes = vec![basic_bread_recipe(), hardtack_recipe()];
        let allocation = allocate_recipes(&facility, &recipes, &stockpile);

        // Basic bread runs first (1 baker -> 1 instance, uses 2 grain, 2 capacity)
        // Then hardtack (2 laborers -> 2 instances, uses 2 grain, 2 capacity)
        assert_eq!(allocation.runs.get(&RecipeId::new(1)), Some(&1));
        assert_eq!(allocation.runs.get(&RecipeId::new(2)), Some(&2));
    }

    #[test]
    fn test_allocate_wrong_facility_type() {
        let workers: HashMap<SkillId, u32> = [(baker(), 2)].into();
        let mut facility = make_facility(workers, 10);
        facility.facility_type = FacilityType::Farm; // Wrong type!
        facility.recipe_priorities = vec![RecipeId::new(1)];

        let mut stockpile = Stockpile::new();
        stockpile.add(GRAIN, 10.0);

        let recipes = vec![basic_bread_recipe()]; // Only works at Bakery
        let allocation = allocate_recipes(&facility, &recipes, &stockpile);

        // Should not run any recipes
        assert!(allocation.runs.is_empty());
    }

    #[test]
    fn test_execute_production() {
        let mut allocation = RecipeAllocation::new(FacilityId::new(1));
        allocation.runs.insert(RecipeId::new(1), 3); // 3 instances of basic bread

        let mut stockpile = Stockpile::new();
        stockpile.add(GRAIN, 10.0);

        let recipes = vec![basic_bread_recipe()];
        let result = execute_production(&allocation, &recipes, &mut stockpile, 1.0);

        // 3 instances * 2 grain = 6 grain consumed
        assert_eq!(result.inputs_consumed.get(&GRAIN), Some(&6.0));
        // 3 instances * 3 bread = 9 bread produced
        assert_eq!(result.outputs_produced.get(&BREAD), Some(&9.0));

        // Stockpile should reflect changes
        assert_eq!(stockpile.get(GRAIN), 4.0); // 10 - 6
        assert_eq!(stockpile.get(BREAD), 9.0); // 0 + 9
    }

    #[test]
    fn test_execute_with_quality_multiplier() {
        let mut allocation = RecipeAllocation::new(FacilityId::new(1));
        allocation.runs.insert(RecipeId::new(1), 2);

        let mut stockpile = Stockpile::new();
        stockpile.add(GRAIN, 10.0);

        let recipes = vec![basic_bread_recipe()];
        // Rich quality = 1.5x output
        let result = execute_production(&allocation, &recipes, &mut stockpile, 1.5);

        // 2 instances * 2 grain = 4 grain consumed (inputs not affected by quality)
        assert_eq!(result.inputs_consumed.get(&GRAIN), Some(&4.0));
        // 2 instances * 3 bread * 1.5 = 9 bread produced
        assert_eq!(result.outputs_produced.get(&BREAD), Some(&9.0));
    }
}
