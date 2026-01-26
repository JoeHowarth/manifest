use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tsify_next::Tsify;

use crate::types::Good;

// ============================================================================
// Goods Market - Per-good market state
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct GoodMarket {
    pub available: f32,   // Goods available for sale
    pub price: f32,       // Current clearing price
    pub last_demand: f32, // Quantity demanded last tick
    pub last_traded: f32, // Quantity actually sold last tick
}

impl Default for GoodMarket {
    fn default() -> Self {
        Self {
            available: 0.0,
            price: 10.0, // Base price
            last_demand: 0.0,
            last_traded: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Market {
    pub goods: HashMap<Good, GoodMarket>,
}

// ============================================================================
// Labor Market - Where wages emerge from supply and demand
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct LaborMarket {
    pub supply: f32, // Workers available
    pub demand: f32, // Workers wanted by facilities
    pub wage: f32,   // Current clearing wage
}

impl Default for LaborMarket {
    fn default() -> Self {
        Self {
            supply: 0.0,
            demand: 0.0,
            wage: 10.0, // Base wage
        }
    }
}

// ============================================================================
// Inventory - Goods held by an entity
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub items: HashMap<Good, f32>,
}

impl Inventory {
    pub fn add(&mut self, good: Good, amount: f32) {
        *self.items.entry(good).or_insert(0.0) += amount;
    }

    pub fn remove(&mut self, good: Good, amount: f32) -> f32 {
        let current = self.items.entry(good).or_insert(0.0);
        let removed = amount.min(*current);
        *current -= removed;
        removed
    }

    pub fn get(&self, good: Good) -> f32 {
        self.items.get(&good).copied().unwrap_or(0.0)
    }
}
