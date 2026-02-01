use std::collections::HashMap;

use crate::pops::types::{GoodId, PopId, Quantity, SettlementId};

// === CONSUMPTION ===

pub struct ConsumptionResult {
    pub actual: HashMap<GoodId, Quantity>,
    pub desired: HashMap<GoodId, Quantity>,
}

// === POP ===

/// A population unit (~100 workers + dependents) bound to a settlement.
/// Makes consumption decisions, participates in labor markets as 1 worker.
#[derive(Debug, Clone)]
pub struct Pop {
    pub id: PopId,
    pub home_settlement: SettlementId,
    pub currency: f64,
    pub stocks: HashMap<GoodId, Quantity>,
    pub desired_consumption_ema: HashMap<GoodId, Quantity>,
    pub need_satisfaction: HashMap<String, f64>,
    /// Smoothed income used as budget for desire discovery and market purchases.
    pub income_ema: f64,
}

impl Pop {
    pub fn new(id: PopId, home_settlement: SettlementId) -> Self {
        Self {
            id,
            home_settlement,
            currency: 1000.0,
            stocks: HashMap::new(),
            desired_consumption_ema: HashMap::new(),
            need_satisfaction: HashMap::new(),
            income_ema: 100.0,
        }
    }

    pub fn with_currency(mut self, currency: f64) -> Self {
        self.currency = currency;
        self
    }

    pub fn with_stocks(mut self, stocks: HashMap<GoodId, Quantity>) -> Self {
        self.stocks = stocks;
        self
    }
}
