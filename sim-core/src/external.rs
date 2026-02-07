use std::collections::HashMap;

use crate::types::{GoodId, Price, Quantity, SettlementId};

/// Config for an anchored good in the outside market.
#[derive(Debug, Clone)]
pub struct AnchoredGoodConfig {
    /// Exogenous reference price for this good in the outside market.
    pub world_price: Price,
    /// Half-band spread in basis points (100 bps = 1%).
    pub spread_bps: f64,
    /// Base outside market depth available each tick.
    pub base_depth: Quantity,
    /// Additional depth per pop at the settlement each tick.
    pub depth_per_pop: Quantity,
    /// Number of price tiers on each side of the outside ladder.
    pub tiers: u32,
    /// Incremental widening per tier in basis points.
    pub tier_step_bps: f64,
}

impl Default for AnchoredGoodConfig {
    fn default() -> Self {
        Self {
            world_price: 10.0,
            spread_bps: 500.0,
            base_depth: 0.0,
            depth_per_pop: 0.5,
            tiers: 9,
            tier_step_bps: 300.0,
        }
    }
}

/// Per-settlement friction and enablement controls for outside trade.
#[derive(Debug, Clone)]
pub struct SettlementFriction {
    pub enabled: bool,
    pub transport_bps: f64,
    pub tariff_bps: f64,
    pub risk_bps: f64,
}

impl Default for SettlementFriction {
    fn default() -> Self {
        Self {
            enabled: false,
            transport_bps: 0.0,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExternalMarketConfig {
    /// Anchored goods and their outside market parameters.
    pub anchors: HashMap<GoodId, AnchoredGoodConfig>,
    /// Settlement-specific friction / enablement toggles.
    pub frictions: HashMap<SettlementId, SettlementFriction>,
}

impl ExternalMarketConfig {
    /// Settlement-level config with disabled default when unset.
    pub fn friction_for(&self, settlement: SettlementId) -> SettlementFriction {
        self.frictions
            .get(&settlement)
            .cloned()
            .unwrap_or_default()
    }
}

/// Aggregate outside flow accounting over simulation runtime.
#[derive(Debug, Clone, Default)]
pub struct OutsideFlowTotals {
    pub imports_qty: HashMap<(SettlementId, GoodId), Quantity>,
    pub exports_qty: HashMap<(SettlementId, GoodId), Quantity>,
    pub imports_value: HashMap<(SettlementId, GoodId), f64>,
    pub exports_value: HashMap<(SettlementId, GoodId), f64>,
}

impl OutsideFlowTotals {
    pub fn record_import(&mut self, settlement: SettlementId, good: GoodId, qty: Quantity, value: f64) {
        *self.imports_qty.entry((settlement, good)).or_insert(0.0) += qty;
        *self.imports_value.entry((settlement, good)).or_insert(0.0) += value;
    }

    pub fn record_export(&mut self, settlement: SettlementId, good: GoodId, qty: Quantity, value: f64) {
        *self.exports_qty.entry((settlement, good)).or_insert(0.0) += qty;
        *self.exports_value.entry((settlement, good)).or_insert(0.0) += value;
    }
}
