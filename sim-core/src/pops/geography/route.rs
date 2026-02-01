// Route type connecting settlements

use crate::pops::types::SettlementId;

/// An edge connecting two settlements
#[derive(Debug, Clone)]
pub struct Route {
    pub from: SettlementId,
    pub to: SettlementId,
    pub distance: u32,     // Travel time in ticks
    pub transport_cost: f64, // Cost per unit of cargo
    pub risk: f64,         // 0.0 - 1.0, chance of incident per trip
}

impl Route {
    pub fn new(from: SettlementId, to: SettlementId, distance: u32) -> Self {
        Self {
            from,
            to,
            distance,
            transport_cost: 1.0,
            risk: 0.0,
        }
    }

    pub fn with_cost(mut self, cost: f64) -> Self {
        self.transport_cost = cost;
        self
    }

    pub fn with_risk(mut self, risk: f64) -> Self {
        self.risk = risk;
        self
    }

    /// Check if this route connects the given settlements (in either direction)
    pub fn connects(&self, a: SettlementId, b: SettlementId) -> bool {
        (self.from == a && self.to == b) || (self.from == b && self.to == a)
    }
}
