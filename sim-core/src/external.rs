use std::collections::HashMap;

use crate::market::{Order, Side};
use crate::types::{GoodId, Price, Quantity, SettlementId};

pub const OUTSIDE_BASE_AGENT_ID: u32 = u32::MAX;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutsideAgentRole {
    ImportSeller,
    ExportBuyer,
}

#[derive(Debug, Clone, Default)]
pub struct OutsideMarketOrders {
    pub orders: Vec<Order>,
    pub budgets: HashMap<u32, f64>,
    pub inventories: HashMap<u32, HashMap<GoodId, f64>>,
    pub roles: HashMap<u32, OutsideAgentRole>,
}

fn import_agent_id(good: GoodId) -> u32 {
    let offset = good.saturating_mul(2).saturating_add(1);
    OUTSIDE_BASE_AGENT_ID.saturating_sub(offset)
}

fn export_agent_id(good: GoodId) -> u32 {
    let offset = good.saturating_mul(2).saturating_add(2);
    OUTSIDE_BASE_AGENT_ID.saturating_sub(offset)
}

/// Build outside import/export ladders for enabled settlement+goods.
pub fn generate_outside_market_orders(
    settlement: SettlementId,
    pop_count: usize,
    config: Option<&ExternalMarketConfig>,
) -> OutsideMarketOrders {
    let Some(config) = config else {
        return OutsideMarketOrders::default();
    };

    let friction = config.friction_for(settlement);
    if !friction.enabled {
        return OutsideMarketOrders::default();
    }

    let mut out = OutsideMarketOrders::default();

    for (&good, anchor) in &config.anchors {
        let tiers = anchor.tiers.max(1);
        let max_depth = anchor.base_depth + anchor.depth_per_pop * pop_count as f64;
        if max_depth <= 0.0 || anchor.world_price <= 0.0 {
            continue;
        }

        let band = (anchor.spread_bps + friction.transport_bps + friction.tariff_bps + friction.risk_bps)
            / 10_000.0;
        let tier_step = anchor.tier_step_bps / 10_000.0;
        let import_agent = import_agent_id(good);
        let export_agent = export_agent_id(good);

        out.roles.insert(import_agent, OutsideAgentRole::ImportSeller);
        out.roles.insert(export_agent, OutsideAgentRole::ExportBuyer);

        let total_weight = (tiers as f64) * (tiers as f64 + 1.0) * 0.5;
        let mut export_budget = 0.0;

        for tier in 0..tiers {
            let tier_weight = (tier + 1) as f64 / total_weight;
            let qty = max_depth * tier_weight;
            if qty <= 0.0 {
                continue;
            }

            let tier_mul = 1.0 + tier_step * tier as f64;
            let import_price = anchor.world_price * (1.0 + band) * tier_mul;
            let export_price = (anchor.world_price * (1.0 - band) / tier_mul).max(0.0001);

            out.orders.push(Order {
                id: 0,
                agent_id: import_agent,
                good,
                side: Side::Sell,
                quantity: qty,
                limit_price: import_price,
            });
            out.orders.push(Order {
                id: 0,
                agent_id: export_agent,
                good,
                side: Side::Buy,
                quantity: qty,
                limit_price: export_price,
            });
            export_budget += qty * export_price;
        }

        out.inventories
            .entry(import_agent)
            .or_default()
            .insert(good, max_depth);
        // Include seller agent in budget table so market relaxation bookkeeping
        // can track its tentative cash flow without panicking.
        out.budgets.insert(import_agent, 0.0);
        out.budgets.insert(export_agent, export_budget);
    }

    out
}
