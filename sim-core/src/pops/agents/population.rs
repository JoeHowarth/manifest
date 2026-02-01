use std::collections::HashMap;

use crate::pops::types::{AgentId, GoodId, Quantity};

// === CONSUMPTION ===

pub struct ConsumptionResult {
    pub actual: HashMap<GoodId, Quantity>,
    pub desired: HashMap<GoodId, Quantity>,
}

// === AGENTS ===

#[derive(Debug, Clone)]
pub struct PopulationState {
    pub id: AgentId,
    pub currency: f64,
    pub stocks: HashMap<GoodId, Quantity>,
    pub desired_consumption_ema: HashMap<GoodId, Quantity>,
    pub need_satisfaction: HashMap<String, f64>,
    /// Smoothed income used as budget for desire discovery and market purchases.
    /// TODO: Update this after income events (wages, sales) with:
    ///   income_ema = 0.8 * income_ema + 0.2 * income_this_tick
    pub income_ema: f64,
}

impl Default for PopulationState {
    fn default() -> Self {
        Self {
            id: 0,
            currency: 1000.0,
            stocks: HashMap::new(),
            desired_consumption_ema: HashMap::new(),
            need_satisfaction: HashMap::new(),
            income_ema: 100.0,
        }
    }
}

impl PopulationState {
    pub fn new(id: AgentId) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    pub fn with_currency(mut self, currency: f64) -> Self {
        self.currency = currency;
        self
    }
}
