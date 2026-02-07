use std::collections::{HashMap, HashSet};

use crate::types::{GoodId, Quantity, SettlementId};
use crate::world::World;

/// World-wide stock snapshot captured at a tick boundary.
#[derive(Debug, Clone, Default)]
pub struct WorldFlowSnapshot {
    pub pop_currency: f64,
    pub merchant_currency: f64,
    pub goods: HashMap<GoodId, Quantity>,
    pub imports_qty: HashMap<GoodId, Quantity>,
    pub exports_qty: HashMap<GoodId, Quantity>,
    pub imports_value: HashMap<GoodId, f64>,
    pub exports_value: HashMap<GoodId, f64>,
}

/// Per-tick stock-flow decomposition output.
#[derive(Debug, Clone, Default)]
pub struct TickStockFlow {
    pub tick: u64,
    pub pop_currency_before: f64,
    pub pop_currency_after: f64,
    pub merchant_currency_before: f64,
    pub merchant_currency_after: f64,
    pub currency_before: f64,
    pub currency_after: f64,
    pub currency_delta: f64,
    pub expected_currency_delta_from_external: f64,
    pub currency_residual: f64,
    pub imports_value_delta: f64,
    pub exports_value_delta: f64,
    pub goods_before: HashMap<GoodId, Quantity>,
    pub goods_after: HashMap<GoodId, Quantity>,
    pub goods_delta: HashMap<GoodId, Quantity>,
    pub imports_qty_delta: HashMap<GoodId, Quantity>,
    pub exports_qty_delta: HashMap<GoodId, Quantity>,
}

fn rollup_by_good<T: Copy + Default + std::ops::AddAssign>(
    keyed: &HashMap<(SettlementId, GoodId), T>,
) -> HashMap<GoodId, T> {
    let mut rolled: HashMap<GoodId, T> = HashMap::new();
    for ((_, good), value) in keyed {
        *rolled.entry(*good).or_default() += *value;
    }
    rolled
}

/// Capture the current world-level stock-flow snapshot.
pub fn capture_world_flow_snapshot(world: &World) -> WorldFlowSnapshot {
    let pop_currency: f64 = world.pops.values().map(|p| p.currency).sum();
    let merchant_currency: f64 = world.merchants.values().map(|m| m.currency).sum();

    let mut goods: HashMap<GoodId, Quantity> = HashMap::new();
    for pop in world.pops.values() {
        for (good, qty) in &pop.stocks {
            *goods.entry(*good).or_insert(0.0) += *qty;
        }
    }
    for merchant in world.merchants.values() {
        for stockpile in merchant.stockpiles.values() {
            for (good, qty) in &stockpile.goods {
                *goods.entry(*good).or_insert(0.0) += *qty;
            }
        }
    }

    WorldFlowSnapshot {
        pop_currency,
        merchant_currency,
        goods,
        imports_qty: rollup_by_good(&world.outside_flow_totals.imports_qty),
        exports_qty: rollup_by_good(&world.outside_flow_totals.exports_qty),
        imports_value: rollup_by_good(&world.outside_flow_totals.imports_value),
        exports_value: rollup_by_good(&world.outside_flow_totals.exports_value),
    }
}

/// Decompose one tick's stock-flow changes using boundary snapshots.
pub fn decompose_tick_flow(
    tick: u64,
    before: &WorldFlowSnapshot,
    after: &WorldFlowSnapshot,
) -> TickStockFlow {
    let pop_currency_before = before.pop_currency;
    let pop_currency_after = after.pop_currency;
    let merchant_currency_before = before.merchant_currency;
    let merchant_currency_after = after.merchant_currency;
    let currency_before = pop_currency_before + merchant_currency_before;
    let currency_after = pop_currency_after + merchant_currency_after;
    let currency_delta = currency_after - currency_before;

    let imports_value_delta: f64 = after
        .imports_value
        .iter()
        .map(|(good, qty_after)| qty_after - before.imports_value.get(good).copied().unwrap_or(0.0))
        .sum();
    let exports_value_delta: f64 = after
        .exports_value
        .iter()
        .map(|(good, qty_after)| qty_after - before.exports_value.get(good).copied().unwrap_or(0.0))
        .sum();
    let expected_currency_delta_from_external = exports_value_delta - imports_value_delta;
    let currency_residual = currency_delta - expected_currency_delta_from_external;

    let mut goods_keys: HashSet<GoodId> = HashSet::new();
    goods_keys.extend(before.goods.keys().copied());
    goods_keys.extend(after.goods.keys().copied());
    let goods_delta: HashMap<GoodId, Quantity> = goods_keys
        .iter()
        .map(|good| {
            let after_qty = after.goods.get(good).copied().unwrap_or(0.0);
            let before_qty = before.goods.get(good).copied().unwrap_or(0.0);
            (*good, after_qty - before_qty)
        })
        .collect();

    let mut import_keys: HashSet<GoodId> = HashSet::new();
    import_keys.extend(before.imports_qty.keys().copied());
    import_keys.extend(after.imports_qty.keys().copied());
    let imports_qty_delta: HashMap<GoodId, Quantity> = import_keys
        .iter()
        .map(|good| {
            let after_qty = after.imports_qty.get(good).copied().unwrap_or(0.0);
            let before_qty = before.imports_qty.get(good).copied().unwrap_or(0.0);
            (*good, after_qty - before_qty)
        })
        .collect();

    let mut export_keys: HashSet<GoodId> = HashSet::new();
    export_keys.extend(before.exports_qty.keys().copied());
    export_keys.extend(after.exports_qty.keys().copied());
    let exports_qty_delta: HashMap<GoodId, Quantity> = export_keys
        .iter()
        .map(|good| {
            let after_qty = after.exports_qty.get(good).copied().unwrap_or(0.0);
            let before_qty = before.exports_qty.get(good).copied().unwrap_or(0.0);
            (*good, after_qty - before_qty)
        })
        .collect();

    TickStockFlow {
        tick,
        pop_currency_before,
        pop_currency_after,
        merchant_currency_before,
        merchant_currency_after,
        currency_before,
        currency_after,
        currency_delta,
        expected_currency_delta_from_external,
        currency_residual,
        imports_value_delta,
        exports_value_delta,
        goods_before: before.goods.clone(),
        goods_after: after.goods.clone(),
        goods_delta,
        imports_qty_delta,
        exports_qty_delta,
    }
}
