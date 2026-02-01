// Settlement type for multi-location economy

use crate::types::{PopId, SettlementId};

/// A node in the trade network
#[derive(Debug, Clone)]
pub struct Settlement {
    pub id: SettlementId,
    pub name: String,
    pub position: (f64, f64),
    pub pop_ids: Vec<PopId>,
    pub natural_resources: Vec<NaturalResource>,
}

impl Settlement {
    pub fn new(id: SettlementId, name: impl Into<String>, position: (f64, f64)) -> Self {
        Self {
            id,
            name: name.into(),
            position,
            pop_ids: Vec::new(),
            natural_resources: Vec::new(),
        }
    }

    pub fn with_pops(mut self, pop_ids: Vec<PopId>) -> Self {
        self.pop_ids = pop_ids;
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
