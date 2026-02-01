// Stockpile type for location-based inventory

use std::collections::HashMap;

use crate::pops::types::{AgentId, GoodId, LocationId, Quantity};

/// Key for stockpile lookup: (agent, location)
pub type StockpileKey = (AgentId, LocationId);

/// Inventory of goods at a specific location owned by an agent
#[derive(Debug, Clone, Default)]
pub struct Stockpile {
    pub goods: HashMap<GoodId, Quantity>,
}

impl Stockpile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, good: GoodId, amount: Quantity) {
        *self.goods.entry(good).or_insert(0.0) += amount;
    }

    pub fn remove(&mut self, good: GoodId, amount: Quantity) -> Quantity {
        let current = self.goods.entry(good).or_insert(0.0);
        let removed = amount.min(*current);
        *current -= removed;
        removed
    }

    pub fn get(&self, good: GoodId) -> Quantity {
        self.goods.get(&good).copied().unwrap_or(0.0)
    }

    pub fn total(&self) -> Quantity {
        self.goods.values().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.goods.values().all(|&q| q <= 0.0)
    }
}
