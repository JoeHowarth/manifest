// Settlement type for multi-location economy

use crate::pops::agents::PopulationState;
use crate::pops::types::SettlementId;

/// A node in the trade network
#[derive(Debug, Clone)]
pub struct Settlement {
    pub id: SettlementId,
    pub name: String,
    pub position: (f64, f64),
    pub population: PopulationState,
    pub natural_resources: Vec<NaturalResource>,
}

impl Settlement {
    pub fn new(id: SettlementId, name: impl Into<String>, position: (f64, f64)) -> Self {
        Self {
            id,
            name: name.into(),
            position,
            population: PopulationState::default(),
            natural_resources: Vec::new(),
        }
    }

    pub fn with_population(mut self, population: PopulationState) -> Self {
        self.population = population;
        self
    }

    pub fn with_resources(mut self, resources: Vec<NaturalResource>) -> Self {
        self.natural_resources = resources;
        self
    }
}

/// What can be extracted at a location
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NaturalResource {
    FertileLand, // Enables grain production
    Fishery,     // Enables fishing
    Forest,      // Enables lumber
    IronOre,     // Enables iron mining
}
