// Recipe definitions for production chains

use std::collections::HashMap;

use crate::labor::SkillId;
use crate::types::{GoodId, Quantity};

use super::FacilityType;

// === RECIPE ID ===

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct RecipeId(pub u32);

impl RecipeId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

// === RECIPE ===

/// A recipe defines how a facility converts inputs to outputs.
///
/// Each recipe instance requires:
/// - A certain amount of facility capacity
/// - Specific workers (by skill)
/// - Input goods
///
/// And produces output goods.
#[derive(Debug, Clone)]
pub struct Recipe {
    pub id: RecipeId,
    pub name: String,
    /// Which facility types can run this recipe
    pub facility_types: Vec<FacilityType>,
    /// Capacity consumed per instance
    pub capacity_cost: u32,
    /// Workers required per instance (by skill)
    pub workers: HashMap<SkillId, u32>,
    /// Input goods consumed per instance
    pub inputs: Vec<(GoodId, Quantity)>,
    /// Output goods produced per instance
    pub outputs: Vec<(GoodId, Quantity)>,
}

impl Recipe {
    pub fn new(id: RecipeId, name: impl Into<String>, facility_types: Vec<FacilityType>) -> Self {
        Self {
            id,
            name: name.into(),
            facility_types,
            capacity_cost: 1,
            workers: HashMap::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }

    pub fn with_capacity_cost(mut self, cost: u32) -> Self {
        self.capacity_cost = cost;
        self
    }

    pub fn with_worker(mut self, skill: SkillId, count: u32) -> Self {
        self.workers.insert(skill, count);
        self
    }

    pub fn with_input(mut self, good: GoodId, qty: Quantity) -> Self {
        self.inputs.push((good, qty));
        self
    }

    pub fn with_output(mut self, good: GoodId, qty: Quantity) -> Self {
        self.outputs.push((good, qty));
        self
    }

    /// Check if this recipe can run on a given facility type
    pub fn can_run_at(&self, facility_type: FacilityType) -> bool {
        self.facility_types.contains(&facility_type)
    }

    /// Check if we have enough workers to run one instance
    pub fn has_workers(&self, available: &HashMap<SkillId, u32>) -> bool {
        self.workers
            .iter()
            .all(|(skill, needed)| available.get(skill).copied().unwrap_or(0) >= *needed)
    }

    /// Check if we have enough inputs to run one instance
    pub fn has_inputs(&self, available: &HashMap<GoodId, Quantity>) -> bool {
        self.inputs
            .iter()
            .all(|(good, needed)| available.get(good).copied().unwrap_or(0.0) >= *needed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laborer() -> SkillId {
        SkillId(1)
    }

    fn baker() -> SkillId {
        SkillId(2)
    }

    const GRAIN: GoodId = 1;
    const BREAD: GoodId = 2;

    #[test]
    fn test_recipe_builder() {
        let recipe = Recipe::new(RecipeId::new(1), "Basic Bread", vec![FacilityType::Bakery])
            .with_capacity_cost(2)
            .with_worker(baker(), 1)
            .with_input(GRAIN, 2.0)
            .with_output(BREAD, 3.0);

        assert_eq!(recipe.name, "Basic Bread");
        assert_eq!(recipe.capacity_cost, 2);
        assert!(recipe.can_run_at(FacilityType::Bakery));
        assert!(!recipe.can_run_at(FacilityType::Farm));
        assert_eq!(recipe.workers.get(&baker()), Some(&1));
        assert_eq!(recipe.inputs.len(), 1);
        assert_eq!(recipe.outputs.len(), 1);
    }

    #[test]
    fn test_has_workers() {
        let recipe = Recipe::new(RecipeId::new(1), "Test", vec![FacilityType::Bakery])
            .with_worker(baker(), 2)
            .with_worker(laborer(), 1);

        // Not enough workers
        let available: HashMap<SkillId, u32> = [(baker(), 1), (laborer(), 1)].into();
        assert!(!recipe.has_workers(&available));

        // Exactly enough
        let available: HashMap<SkillId, u32> = [(baker(), 2), (laborer(), 1)].into();
        assert!(recipe.has_workers(&available));

        // More than enough
        let available: HashMap<SkillId, u32> = [(baker(), 5), (laborer(), 3)].into();
        assert!(recipe.has_workers(&available));
    }

    #[test]
    fn test_has_inputs() {
        let recipe = Recipe::new(RecipeId::new(1), "Test", vec![FacilityType::Bakery])
            .with_input(GRAIN, 2.0);

        // Not enough
        let available: HashMap<GoodId, Quantity> = [(GRAIN, 1.5)].into();
        assert!(!recipe.has_inputs(&available));

        // Exactly enough
        let available: HashMap<GoodId, Quantity> = [(GRAIN, 2.0)].into();
        assert!(recipe.has_inputs(&available));

        // More than enough
        let available: HashMap<GoodId, Quantity> = [(GRAIN, 10.0)].into();
        assert!(recipe.has_inputs(&available));
    }
}
