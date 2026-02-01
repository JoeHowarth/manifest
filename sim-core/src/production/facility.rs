// Facility type and definition for production

use std::collections::HashMap;

use crate::geography::ResourceType;
use crate::labor::SkillId;
use crate::types::{FacilityId, MerchantId, SettlementId};

use super::RecipeId;

// === FACILITY TYPE ===

/// What kind of facility this is
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FacilityType {
    // Primary production (require natural resources)
    Farm,     // Land → grain
    Fishery,  // Coastal → fish
    Sawmill,  // Forest → lumber
    IronMine, // OreDeposit → iron

    // Secondary production (no natural resource required)
    Bakery, // grain → bread
    Smithy, // iron → tools
}

// === FACILITY DEFINITION (CONTENT) ===

/// Immutable definition of a facility type.
/// This is content/configuration, not game state.
#[derive(Debug, Clone)]
pub struct FacilityDef {
    pub facility_type: FacilityType,
    pub name: String,
    /// Natural resource required (None for secondary production)
    pub required_resource: Option<ResourceType>,
    /// Base capacity for production
    pub base_capacity: u32,
    /// Cost to construct
    pub construction_cost: f64,
    /// Fraction of construction cost recovered on demolition
    pub salvage_fraction: f64,
}

impl FacilityDef {
    pub fn new(facility_type: FacilityType, name: impl Into<String>) -> Self {
        Self {
            facility_type,
            name: name.into(),
            required_resource: None,
            base_capacity: 10,
            construction_cost: 100.0,
            salvage_fraction: 0.3,
        }
    }

    pub fn with_resource(mut self, resource: ResourceType) -> Self {
        self.required_resource = Some(resource);
        self
    }

    pub fn with_capacity(mut self, capacity: u32) -> Self {
        self.base_capacity = capacity;
        self
    }

    pub fn with_construction_cost(mut self, cost: f64) -> Self {
        self.construction_cost = cost;
        self
    }

    pub fn with_salvage_fraction(mut self, fraction: f64) -> Self {
        self.salvage_fraction = fraction;
        self
    }

    /// Is this a primary production facility (requires natural resource)?
    pub fn is_primary(&self) -> bool {
        self.required_resource.is_some()
    }
}

/// Get the default definitions for all facility types.
/// In the future this could be loaded from data files.
pub fn get_facility_defs() -> Vec<FacilityDef> {
    vec![
        // Primary production
        FacilityDef::new(FacilityType::Farm, "Farm")
            .with_resource(ResourceType::Land)
            .with_capacity(10)
            .with_construction_cost(200.0),
        FacilityDef::new(FacilityType::Fishery, "Fishery")
            .with_resource(ResourceType::Coastal)
            .with_capacity(8)
            .with_construction_cost(150.0),
        FacilityDef::new(FacilityType::Sawmill, "Sawmill")
            .with_resource(ResourceType::Forest)
            .with_capacity(10)
            .with_construction_cost(180.0),
        FacilityDef::new(FacilityType::IronMine, "Iron Mine")
            .with_resource(ResourceType::OreDeposit)
            .with_capacity(6)
            .with_construction_cost(300.0),
        // Secondary production
        FacilityDef::new(FacilityType::Bakery, "Bakery")
            .with_capacity(8)
            .with_construction_cost(120.0),
        FacilityDef::new(FacilityType::Smithy, "Smithy")
            .with_capacity(6)
            .with_construction_cost(250.0),
    ]
}

/// Look up a facility definition by type
pub fn get_facility_def(facility_type: FacilityType) -> Option<FacilityDef> {
    get_facility_defs()
        .into_iter()
        .find(|def| def.facility_type == facility_type)
}

// === FACILITY INSTANCE (GAME STATE) ===

/// A production facility at a settlement.
/// This is mutable game state.
#[derive(Debug, Clone)]
pub struct Facility {
    pub id: FacilityId,
    pub facility_type: FacilityType,
    pub settlement: SettlementId,
    pub owner: MerchantId,

    /// Capacity for running recipes
    pub capacity: u32,

    /// Index into settlement.resource_slots (for primary facilities)
    pub resource_slot_index: Option<usize>,

    /// Currency available for paying wages (facility treasury)
    pub currency: f64,

    /// Current employees by primary skill
    pub workers: HashMap<SkillId, u32>,

    /// Recipe priorities - first recipe has highest priority
    pub recipe_priorities: Vec<RecipeId>,
}

impl Facility {
    pub fn new(
        id: FacilityId,
        facility_type: FacilityType,
        settlement: SettlementId,
        owner: MerchantId,
    ) -> Self {
        let capacity = get_facility_def(facility_type)
            .map(|def| def.base_capacity)
            .unwrap_or(10);

        Self {
            id,
            facility_type,
            settlement,
            owner,
            capacity,
            resource_slot_index: None,
            currency: 0.0,
            workers: HashMap::new(),
            recipe_priorities: Vec::new(),
        }
    }

    pub fn with_currency(mut self, currency: f64) -> Self {
        self.currency = currency;
        self
    }

    pub fn with_resource_slot(mut self, slot_index: usize) -> Self {
        self.resource_slot_index = Some(slot_index);
        self
    }

    pub fn with_recipe_priorities(mut self, priorities: Vec<RecipeId>) -> Self {
        self.recipe_priorities = priorities;
        self
    }

    /// Total workers across all skills
    pub fn total_workers(&self) -> u32 {
        self.workers.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_facility_defs() {
        let defs = get_facility_defs();
        assert_eq!(defs.len(), 6);

        // Check primary facilities have resources
        let farm = get_facility_def(FacilityType::Farm).unwrap();
        assert!(farm.is_primary());
        assert_eq!(farm.required_resource, Some(ResourceType::Land));

        // Check secondary facilities don't
        let bakery = get_facility_def(FacilityType::Bakery).unwrap();
        assert!(!bakery.is_primary());
        assert_eq!(bakery.required_resource, None);
    }

    #[test]
    fn test_facility_gets_capacity_from_def() {
        let facility = Facility::new(
            FacilityId::new(1),
            FacilityType::Farm,
            SettlementId::new(1),
            MerchantId::new(1),
        );

        // Should get capacity from FacilityDef
        assert_eq!(facility.capacity, 10);
    }
}
