use std::collections::HashMap;

use crate::pops::agents::{MerchantAgent, Pop};
use crate::pops::consumption;
use crate::pops::market::{self, Order, Side};
use crate::pops::needs::Need;
use crate::pops::types::{AgentId, GoodId, GoodProfile, Price, SettlementId};

// === CONSTANTS ===

const BUFFER_TICKS: f64 = 5.0;
const PRICE_SWEEP_MIN: f64 = 0.6;
const PRICE_SWEEP_MAX: f64 = 1.4;
const PRICE_SWEEP_POINTS: usize = 9;

// === DEMAND CURVE FUNCTIONS ===

/// Quantity demanded as fraction of desired_ema.
///
/// - norm_p: price / clearing_price_ema (1.0 = at EMA)
/// - norm_c: current_stock / target (1.0 = at target)
///
/// Returns value in [0, 1] representing fraction of desired_ema to buy.
fn qty_norm(norm_p: f64, norm_c: f64) -> f64 {
    let shortfall = (1.0 - norm_c).max(0.0);
    let price_factor = 1.0 - norm_p;
    (shortfall * (0.3 + 0.7 * price_factor)).clamp(0.0, 1.0)
}

/// Quantity supplied as fraction of desired_ema.
/// Inverts both inputs to reuse qty_norm logic for supply curve.
fn qty_sell(norm_p: f64, norm_c: f64) -> f64 {
    qty_norm(1.0 / norm_p, 1.0 / norm_c)
}

// === ORDER GENERATION ===

/// Generate demand curve orders for a population.
/// Sweeps across price points and generates orders at each level.
fn generate_demand_curve_orders(
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
            for i in 0..PRICE_SWEEP_POINTS {
                let norm_p = PRICE_SWEEP_MIN
                    + (PRICE_SWEEP_MAX - PRICE_SWEEP_MIN) * (i as f64)
                        / ((PRICE_SWEEP_POINTS - 1) as f64);
                let qty_frac = qty_norm(norm_p, norm_c);
                let qty = qty_frac * desired_ema;

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
            let sell_min = 0.7;
            let sell_max = 1.0 / PRICE_SWEEP_MIN; // ~1.67

            for i in 0..PRICE_SWEEP_POINTS {
                let norm_p = sell_min
                    + (sell_max - sell_min) * (i as f64) / ((PRICE_SWEEP_POINTS - 1) as f64);
                let qty_frac = qty_sell(norm_p, norm_c);
                let qty = qty_frac * desired_ema;

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
pub fn run_settlement_tick(
    settlement: SettlementId,
    pops: &mut [&mut Pop],
    merchants: &mut [&mut MerchantAgent],
    good_profiles: &[GoodProfile],
    needs: &HashMap<String, Need>,
    price_ema: &mut HashMap<GoodId, Price>,
) -> market::MultiMarketResult {
    // 0. Production

    // 1. CONSUMPTION PHASE
    for pop in pops.iter_mut() {
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
        for (good, qty) in result.actual {
            *pop.stocks.entry(good).or_insert(0.0) -= qty;
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
        }
        all_orders.extend(orders);
    }

    for merchant in merchants.iter() {
        let mut orders = merchant.generate_orders(price_ema);
        for o in &mut orders {
            o.id = next_order_id;
            next_order_id += 1;
        }
        all_orders.extend(orders);
    }

    // 3. GATHER BUDGETS
    // Pops spend up to income_ema per tick (but not more than they have)
    // Extra coins accumulate as savings
    let budgets: HashMap<AgentId, f64> = pops
        .iter()
        .map(|p| (p.id.0, p.income_ema.min(p.currency)))
        .chain(merchants.iter().map(|m| (m.id.0, m.currency)))
        .collect();

    // 4. MARKET CLEARING
    let good_ids: Vec<_> = good_profiles.iter().map(|p| p.good).collect();
    let result = market::clear_multi_market(
        &good_ids,
        all_orders,
        &budgets,
        20,
        market::PriceBias::FavorSellers,
    );

    // 5. APPLY FILLS
    for fill in &result.fills {
        if let Some(pop) = pops.iter_mut().find(|p| p.id.0 == fill.agent_id) {
            market::apply_fill(pop, fill);
        } else if let Some(merchant) = merchants.iter_mut().find(|m| m.id.0 == fill.agent_id) {
            market::apply_fill_merchant(merchant, settlement, fill);
        }
    }

    // 6. UPDATE PRICE EMA
    for (good, price) in &result.clearing_prices {
        let ema = price_ema.entry(*good).or_insert(*price);
        *ema = 0.7 * *ema + 0.3 * price;
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
        dummy_settlement,
        &mut pop_refs,
        &mut merchant_refs,
        good_profiles,
        needs,
        price_ema,
    )
}
