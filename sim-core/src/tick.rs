use std::collections::HashMap;

use crate::agents::{MerchantAgent, Pop};
use crate::consumption;
use crate::external::{
    ExternalMarketConfig, OutsideAgentRole, OutsideFlowTotals, generate_outside_market_orders,
};
use crate::labor::{SubsistenceReservationConfig, ranked_subsistence_yields};
use crate::market::{self, Order, Side};
use crate::needs::Need;
use crate::types::{AgentId, GoodId, GoodProfile, PopId, Price, SettlementId};

// === CONSTANTS ===

pub const BUFFER_TICKS: f64 = 5.0;
pub const PRICE_SWEEP_MIN: f64 = 0.6;
pub const PRICE_SWEEP_MAX: f64 = 1.4;
pub const PRICE_SWEEP_POINTS: usize = 9;
pub const PRICE_EMA_ALPHA: f64 = 0.3;
pub const EXTERNAL_EMA_WEIGHT_MAX: f64 = 0.2;
pub const EXTERNAL_EMA_WEIGHT_NO_TRADE_MAX: f64 = 0.35;
pub const EXTERNAL_EMA_DEPTH_HALF_SATURATION: f64 = 20.0;

// === DEMAND CURVE FUNCTIONS ===

/// Quantity demanded as fraction of desired_ema.
///
/// - norm_p: price / clearing_price_ema (1.0 = at EMA)
/// - norm_c: current_stock / target (1.0 = at target)
///
/// Returns value in [0, 1] representing fraction of desired_ema to buy.
pub fn qty_norm(norm_p: f64, norm_c: f64) -> f64 {
    let shortfall = (1.0 - norm_c).max(0.0);
    let price_factor = 1.0 - norm_p;
    (shortfall * (0.3 + 0.7 * price_factor)).clamp(0.0, 1.0)
}

/// Quantity supplied as fraction of desired_ema.
/// Inverts both inputs to reuse qty_norm logic for supply curve.
pub fn qty_sell(norm_p: f64, norm_c: f64) -> f64 {
    qty_norm(1.0 / norm_p, 1.0 / norm_c)
}

fn anchored_external_ref_and_weight(
    settlement: SettlementId,
    pop_count: usize,
    good: GoodId,
    current_ema: Price,
    local_price: Option<Price>,
    external_market: Option<&ExternalMarketConfig>,
    no_local_trade: bool,
) -> Option<(Price, f64)> {
    let config = external_market?;
    let friction = config.friction_for(settlement);
    if !friction.enabled {
        return None;
    }

    let anchor = config.anchors.get(&good)?;
    if anchor.world_price <= 0.0 {
        return None;
    }
    let band =
        (anchor.spread_bps + friction.transport_bps + friction.tariff_bps + friction.risk_bps)
            / 10_000.0;
    let import_edge = anchor.world_price * (1.0 + band);
    let export_edge = (anchor.world_price * (1.0 - band)).max(0.0001);
    let midpoint = 0.5 * (import_edge + export_edge);

    let ext_ref = if let Some(local) = local_price {
        if local >= midpoint {
            import_edge
        } else {
            export_edge
        }
    } else if current_ema >= midpoint {
        import_edge
    } else {
        export_edge
    };

    let depth = (anchor.base_depth + anchor.depth_per_pop * pop_count as f64).max(0.0);
    let depth_signal = if depth <= 0.0 {
        0.0
    } else {
        depth / (depth + EXTERNAL_EMA_DEPTH_HALF_SATURATION)
    };
    let max_weight = if no_local_trade {
        EXTERNAL_EMA_WEIGHT_NO_TRADE_MAX
    } else {
        EXTERNAL_EMA_WEIGHT_MAX
    };
    let weight = (max_weight * depth_signal).clamp(0.0, max_weight);

    Some((ext_ref, weight))
}

// === ORDER GENERATION ===

/// Generate demand curve orders for a population.
/// Sweeps across price points and generates orders at each level.
pub fn generate_demand_curve_orders(
    pop: &Pop,
    good_profiles: &[GoodProfile],
    price_ema: &HashMap<GoodId, Price>,
) -> Vec<Order> {
    let mut orders = Vec::new();

    for profile in good_profiles {
        let good = profile.good;
        let ema_price = price_ema.get(&good).copied().unwrap_or(1.0);
        let current_stock = pop.stocks.get(&good).copied().unwrap_or(0.0);
        let desired_ema = pop
            .desired_consumption_ema
            .get(&good)
            .copied()
            .unwrap_or(1.0);

        let target = desired_ema * BUFFER_TICKS;

        if target <= 0.0 {
            continue;
        }

        let norm_c = current_stock / target;

        if current_stock < target {
            // Buying mode: sweep prices from low to high
            // Quantity is fraction of TARGET so we buy enough to reach buffer
            for i in 0..PRICE_SWEEP_POINTS {
                let norm_p = PRICE_SWEEP_MIN
                    + (PRICE_SWEEP_MAX - PRICE_SWEEP_MIN) * (i as f64)
                        / ((PRICE_SWEEP_POINTS - 1) as f64);
                let qty_frac = qty_norm(norm_p, norm_c);
                let qty = qty_frac * target;

                if qty > 0.001 {
                    orders.push(Order {
                        id: 0, // assigned later
                        agent_id: pop.id.0,
                        good,
                        side: Side::Buy,
                        quantity: qty,
                        limit_price: norm_p * ema_price,
                    });
                }
            }
        } else {
            // Selling mode: sweep prices from low to high
            // Supply curve: higher price = more willing to sell
            // Quantity is fraction of excess above target
            let sell_min = 0.7;
            let sell_max = 1.0 / PRICE_SWEEP_MIN; // ~1.67
            let excess = current_stock - target;

            for i in 0..PRICE_SWEEP_POINTS {
                let norm_p = sell_min
                    + (sell_max - sell_min) * (i as f64) / ((PRICE_SWEEP_POINTS - 1) as f64);
                let qty_frac = qty_sell(norm_p, norm_c);
                let qty = qty_frac * excess;

                if qty > 0.001 {
                    orders.push(Order {
                        id: 0, // assigned later
                        agent_id: pop.id.0,
                        good,
                        side: Side::Sell,
                        quantity: qty,
                        limit_price: norm_p * ema_price,
                    });
                }
            }
        }
    }

    orders
}

// === FULL TICK ===

/*
Consumption
- remove actual
- blend desired

Production
- pay wages
- consume inputs
- produce outputs

Market
- Generate Pop Supply/Demand Curves -> Orders
- Leader / Merchant Orders
- Gather Budgets
- Iterative multi-market clearing
- Apply fills
- Update price ema

Labor Market - matches pops with facilities
- Facilities calc demand curve for num workers hired using mvp + profit margin
-


Update Price and Income EMA


*/

/// Run a market tick for a single settlement.
/// Pops at this settlement participate in consumption and trading.
/// Merchants with presence at this settlement participate in trading.
#[allow(unused_variables, clippy::too_many_arguments)]
pub fn run_settlement_tick(
    tick: u64,
    settlement: SettlementId,
    pops: &mut [&mut Pop],
    merchants: &mut [&mut MerchantAgent],
    good_profiles: &[GoodProfile],
    needs: &HashMap<String, Need>,
    price_ema: &mut HashMap<GoodId, Price>,
    external_market: Option<&ExternalMarketConfig>,
    outside_flow_totals: Option<&mut OutsideFlowTotals>,
    subsistence_config: Option<&SubsistenceReservationConfig>,
) -> market::MultiMarketResult {
    // 0. Production

    // 0.5 SUBSISTENCE PHASE (in-kind fallback for unemployed pops)
    if let Some(cfg) = subsistence_config {
        let unemployed_ids: Vec<PopId> = pops
            .iter()
            .filter(|p| p.employed_at.is_none())
            .map(|p| p.id)
            .collect();

        let yields = ranked_subsistence_yields(&unemployed_ids, cfg.q_max, cfg.crowding_alpha);
        let yield_map: HashMap<PopId, f64> = yields.into_iter().collect();

        for pop in pops.iter_mut() {
            if pop.employed_at.is_some() {
                continue;
            }
            let qty = yield_map.get(&pop.id).copied().unwrap_or(0.0);
            if qty <= 0.0 {
                continue;
            }

            *pop.stocks.entry(cfg.grain_good).or_insert(0.0) += qty;

            #[cfg(feature = "instrument")]
            tracing::info!(
                target: "subsistence",
                tick = tick,
                settlement_id = settlement.0,
                pop_id = pop.id.0,
                good_id = cfg.grain_good,
                quantity = qty,
            );
        }
    }

    // 1. CONSUMPTION PHASE
    for pop in pops.iter_mut() {
        // Reset need satisfaction for this tick (it's per-tick, not cumulative)
        pop.need_satisfaction.clear();

        let result = consumption::compute_consumption(
            &pop.stocks,
            good_profiles,
            needs,
            &mut pop.need_satisfaction,
            price_ema,
            pop.income_ema,
            &pop.desired_consumption_ema,
        );

        // Subtract actual consumption from stocks
        for (good, qty) in &result.actual {
            let stock_before = pop.stocks.get(good).copied().unwrap_or(0.0);
            *pop.stocks.entry(*good).or_insert(0.0) -= qty;
            let stock_after = pop.stocks.get(good).copied().unwrap_or(0.0);

            #[cfg(feature = "instrument")]
            {
                let desired = result.desired.get(good).copied().unwrap_or(0.0);
                tracing::info!(
                    target: "consumption",
                    tick = tick,
                    pop_id = pop.id.0,
                    good_id = *good,
                    desired = desired,
                    actual = *qty,
                    stock_before = stock_before,
                    stock_after = stock_after,
                );
            }
            let _ = (stock_before, stock_after); // Suppress unused warnings
        }

        // Blend desired into EMA
        for (good, qty) in result.desired {
            let ema = pop.desired_consumption_ema.entry(good).or_insert(qty);
            *ema = 0.8 * *ema + 0.2 * qty;
        }
    }

    // 2. ORDER GENERATION
    let mut all_orders = Vec::new();
    let mut next_order_id = 0u64;

    for pop in pops.iter() {
        let mut orders = generate_demand_curve_orders(pop, good_profiles, price_ema);
        for o in &mut orders {
            o.id = next_order_id;
            next_order_id += 1;

            #[cfg(feature = "instrument")]
            {
                let side_str = match o.side {
                    market::Side::Buy => "buy",
                    market::Side::Sell => "sell",
                };
                tracing::info!(
                    target: "order",
                    tick = tick,
                    settlement_id = settlement.0,
                    order_id = o.id,
                    agent_id = o.agent_id,
                    agent_type = "pop",
                    good_id = o.good,
                    side = side_str,
                    quantity = o.quantity,
                    limit_price = o.limit_price,
                );
            }
        }
        all_orders.extend(orders);
    }

    for merchant in merchants.iter() {
        let mut orders = merchant.generate_orders(settlement, price_ema);
        for o in &mut orders {
            o.id = next_order_id;
            next_order_id += 1;

            #[cfg(feature = "instrument")]
            {
                let side_str = match o.side {
                    market::Side::Buy => "buy",
                    market::Side::Sell => "sell",
                };
                tracing::info!(
                    target: "order",
                    tick = tick,
                    settlement_id = settlement.0,
                    order_id = o.id,
                    agent_id = o.agent_id,
                    agent_type = "merchant",
                    good_id = o.good,
                    side = side_str,
                    quantity = o.quantity,
                    limit_price = o.limit_price,
                );
            }
        }
        all_orders.extend(orders);
    }

    // Inject outside market ladders (if enabled for this settlement)
    let outside_market = generate_outside_market_orders(settlement, pops.len(), external_market);
    for mut order in outside_market.orders {
        order.id = next_order_id;
        next_order_id += 1;

        #[cfg(feature = "instrument")]
        {
            let side_str = match order.side {
                market::Side::Buy => "buy",
                market::Side::Sell => "sell",
            };
            tracing::info!(
                target: "order",
                tick = tick,
                settlement_id = settlement.0,
                order_id = order.id,
                agent_id = order.agent_id,
                agent_type = "outside",
                good_id = order.good,
                side = side_str,
                quantity = order.quantity,
                limit_price = order.limit_price,
            );
        }

        all_orders.push(order);
    }

    // 3. GATHER BUDGETS
    // Pops spend up to income_ema per tick (but not more than they have)
    // Extra coins accumulate as savings
    let mut budgets: HashMap<AgentId, f64> = pops
        .iter()
        .map(|p| (p.id.0, p.income_ema.min(p.currency)))
        .chain(merchants.iter().map(|m| (m.id.0, m.currency)))
        .collect();
    for (agent, budget) in outside_market.budgets {
        budgets.insert(agent, budget);
    }

    // 4. GATHER SELLER INVENTORIES
    // Sellers can only sell what they actually have in stock.
    // Include both pops and merchants so per-agent sell ladders cannot overfill.
    let good_ids: Vec<_> = good_profiles.iter().map(|p| p.good).collect();
    let mut seller_inventories: HashMap<AgentId, HashMap<GoodId, f64>> = pops
        .iter()
        .map(|p| {
            let inv: HashMap<GoodId, f64> = good_ids
                .iter()
                .map(|&g| (g, p.stocks.get(&g).copied().unwrap_or(0.0)))
                .collect();
            (p.id.0, inv)
        })
        .chain(merchants.iter().map(|m| {
            let inv: HashMap<GoodId, f64> = m
                .stockpiles
                .get(&settlement)
                .map(|stockpile| good_ids.iter().map(|&g| (g, stockpile.get(g))).collect())
                .unwrap_or_default();
            (m.id.0, inv)
        }))
        .collect();
    for (agent, inv) in outside_market.inventories {
        let entry = seller_inventories.entry(agent).or_default();
        for (good, qty) in inv {
            *entry.entry(good).or_insert(0.0) += qty;
        }
    }

    // 5. MARKET CLEARING
    let price_bias = if outside_market.roles.is_empty() {
        market::PriceBias::FavorSellers
    } else {
        // With outside ladders active, adapt tie breaks to local imbalance:
        // shortage -> favor buyers (import-cap preserving), surplus -> favor
        // sellers (export-floor preserving), near-balance -> neutral.
        let (inside_buy, inside_sell) = all_orders
            .iter()
            .filter(|o| !outside_market.roles.contains_key(&o.agent_id))
            .fold((0.0, 0.0), |(b, s), o| match o.side {
                Side::Buy => (b + o.quantity, s),
                Side::Sell => (b, s + o.quantity),
            });
        if inside_buy > inside_sell + 1e-12 {
            market::PriceBias::FavorBuyers
        } else if inside_sell > inside_buy + 1e-12 {
            market::PriceBias::FavorSellers
        } else {
            market::PriceBias::Neutral
        }
    };

    let result = market::clear_multi_market(
        &good_ids,
        all_orders,
        &budgets,
        Some(&seller_inventories),
        20,
        price_bias,
    );

    // 5. APPLY FILLS
    let mut outside_flow_totals = outside_flow_totals;
    for fill in &result.fills {
        if let Some(role) = outside_market.roles.get(&fill.agent_id).copied() {
            let value = fill.quantity * fill.price;
            match role {
                OutsideAgentRole::ImportSeller => {
                    if let Some(totals) = outside_flow_totals.as_deref_mut() {
                        totals.record_import(settlement, fill.good, fill.quantity, value);
                    }
                    #[cfg(feature = "instrument")]
                    tracing::info!(
                        target: "external_flow",
                        tick = tick,
                        settlement_id = settlement.0,
                        flow = "import",
                        good_id = fill.good,
                        quantity = fill.quantity,
                        value = value,
                    );
                }
                OutsideAgentRole::ExportBuyer => {
                    if let Some(totals) = outside_flow_totals.as_deref_mut() {
                        totals.record_export(settlement, fill.good, fill.quantity, value);
                    }
                    #[cfg(feature = "instrument")]
                    tracing::info!(
                        target: "external_flow",
                        tick = tick,
                        settlement_id = settlement.0,
                        flow = "export",
                        good_id = fill.good,
                        quantity = fill.quantity,
                        value = value,
                    );
                }
            }
            continue;
        }

        let is_pop = if let Some(pop) = pops.iter_mut().find(|p| p.id.0 == fill.agent_id) {
            market::apply_fill(pop, fill);
            true
        } else if let Some(merchant) = merchants.iter_mut().find(|m| m.id.0 == fill.agent_id) {
            market::apply_fill_merchant(merchant, settlement, fill);
            false
        } else {
            false
        };

        #[cfg(feature = "instrument")]
        {
            let side_str = match fill.side {
                market::Side::Buy => "buy",
                market::Side::Sell => "sell",
            };
            let agent_type = if is_pop { "pop" } else { "merchant" };
            tracing::info!(
                target: "fill",
                tick = tick,
                settlement_id = settlement.0,
                agent_id = fill.agent_id,
                agent_type = agent_type,
                good_id = fill.good,
                side = side_str,
                quantity = fill.quantity,
                price = fill.price,
            );
        }
        let _ = is_pop; // Suppress unused warning when feature disabled
    }

    // 6. UPDATE PRICE EMA
    for &good in &good_ids {
        let local_price = result.clearing_prices.get(&good).copied();
        let no_local_trade = local_price.is_none();

        let ema = if let Some(p) = local_price {
            price_ema.entry(good).or_insert(p)
        } else {
            price_ema.entry(good).or_insert(1.0)
        };

        let observed_price = if let Some(local) = local_price {
            if let Some((ext_ref, w_ext)) = anchored_external_ref_and_weight(
                settlement,
                pops.len(),
                good,
                *ema,
                Some(local),
                external_market,
                false,
            ) {
                (1.0 - w_ext) * local + w_ext * ext_ref
            } else {
                local
            }
        } else if let Some((ext_ref, w_ext)) = anchored_external_ref_and_weight(
            settlement,
            pops.len(),
            good,
            *ema,
            None,
            external_market,
            no_local_trade,
        ) {
            (1.0 - w_ext) * *ema + w_ext * ext_ref
        } else {
            *ema
        };

        *ema = (1.0 - PRICE_EMA_ALPHA) * *ema + PRICE_EMA_ALPHA * observed_price;
    }

    result
}

// === LEGACY API ===

/// Legacy API for running a market tick without settlement context.
/// Merchants will use a dummy settlement for stockpile access.
#[deprecated(note = "Use run_settlement_tick instead for proper per-settlement markets")]
pub fn run_market_tick(
    populations: &mut [Pop],
    merchants: &mut [MerchantAgent],
    good_profiles: &[GoodProfile],
    needs: &HashMap<String, Need>,
    price_ema: &mut HashMap<GoodId, Price>,
) -> market::MultiMarketResult {
    // Create a dummy settlement for legacy compatibility
    let dummy_settlement = SettlementId::new(0);

    // Convert to the format expected by run_settlement_tick
    let mut pop_refs: Vec<&mut Pop> = populations.iter_mut().collect();
    let mut merchant_refs: Vec<&mut MerchantAgent> = merchants.iter_mut().collect();

    run_settlement_tick(
        0, // Legacy API doesn't track tick
        dummy_settlement,
        &mut pop_refs,
        &mut merchant_refs,
        good_profiles,
        needs,
        price_ema,
        None,
        None,
        None,
    )
}
