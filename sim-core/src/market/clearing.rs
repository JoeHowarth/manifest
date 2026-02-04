use std::collections::HashMap;

use crate::agents::{MerchantAgent, Pop};
use crate::types::{AgentId, GoodId, Price, SettlementId};

use super::orders::{Fill, Order, Side};

// === SINGLE MARKET CLEARING ===

#[derive(Clone, Copy, Debug, Default)]
pub enum PriceBias {
    #[default]
    FavorSellers, // highest price at max volume
    FavorBuyers, // lowest price at max volume
}

pub struct MarketClearResult {
    pub clearing_price: Option<Price>, // None if no trade
    pub fills: Vec<Fill>,
}

/// Clear one good's market via call auction
pub fn clear_single_market(
    good: GoodId,
    orders: &[Order], // filtered to this good
    bias: PriceBias,
) -> MarketClearResult {
    let mut buys: Vec<_> = orders
        .iter()
        .filter(|o| matches!(o.side, Side::Buy))
        .collect();
    let mut sells: Vec<_> = orders
        .iter()
        .filter(|o| matches!(o.side, Side::Sell))
        .collect();

    if buys.is_empty() || sells.is_empty() {
        return MarketClearResult {
            clearing_price: None,
            fills: Vec::new(),
        };
    }

    // Sort buys descending by price (highest bidder first)
    buys.sort_by(|a, b| b.limit_price.partial_cmp(&a.limit_price).unwrap());
    // Sort sells ascending by price (lowest ask first)
    sells.sort_by(|a, b| a.limit_price.partial_cmp(&b.limit_price).unwrap());

    // Find clearing price: price that maximizes traded volume
    let mut clearing_price = None;
    let mut max_volume = 0.0;

    // Collect all price points
    let mut price_points: Vec<Price> = buys
        .iter()
        .map(|o| o.limit_price)
        .chain(sells.iter().map(|o| o.limit_price))
        .collect();
    match bias {
        PriceBias::FavorSellers => price_points.sort_by(|a, b| b.partial_cmp(a).unwrap()),
        PriceBias::FavorBuyers => price_points.sort_by(|a, b| a.partial_cmp(b).unwrap()),
    }
    price_points.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

    for price in price_points {
        // Demand at this price: all buys with limit >= price
        let demand: f64 = buys
            .iter()
            .filter(|o| o.limit_price >= price)
            .map(|o| o.quantity)
            .sum();
        // Supply at this price: all sells with limit <= price
        let supply: f64 = sells
            .iter()
            .filter(|o| o.limit_price <= price)
            .map(|o| o.quantity)
            .sum();

        let volume = demand.min(supply);
        if volume > max_volume {
            max_volume = volume;
            clearing_price = Some(price);
        }
    }

    let Some(price) = clearing_price else {
        return MarketClearResult {
            clearing_price: None,
            fills: Vec::new(),
        };
    };

    // Generate fills at clearing price
    let mut fills = Vec::new();
    let mut remaining_demand = max_volume;
    let mut remaining_supply = max_volume;

    // Fill buys with proportional allocation at same price levels
    // Group buys by price level (descending)
    let mut buy_idx = 0;
    while buy_idx < buys.len() && remaining_demand > 0.0 {
        let current_price = buys[buy_idx].limit_price;
        if current_price < price {
            break;
        }

        // Collect all buys at this price level
        let mut price_level_buys: Vec<&Order> = Vec::new();
        let mut price_level_demand = 0.0;
        while buy_idx < buys.len() && (buys[buy_idx].limit_price - current_price).abs() < 1e-9 {
            if buys[buy_idx].limit_price >= price {
                price_level_buys.push(buys[buy_idx]);
                price_level_demand += buys[buy_idx].quantity;
            }
            buy_idx += 1;
        }

        if price_level_buys.is_empty() {
            continue;
        }

        // Proportional allocation at this price level
        let fill_ratio = if price_level_demand > remaining_demand {
            remaining_demand / price_level_demand
        } else {
            1.0
        };

        for buy in price_level_buys {
            let fill_qty = buy.quantity * fill_ratio;
            if fill_qty > 0.0 {
                remaining_demand -= fill_qty;
                fills.push(Fill {
                    order_id: buy.id,
                    agent_id: buy.agent_id,
                    good,
                    side: Side::Buy,
                    quantity: fill_qty,
                    price,
                });
            }
        }
    }

    // Fill sells with proportional allocation at same price levels
    let mut sell_idx = 0;
    while sell_idx < sells.len() && remaining_supply > 0.0 {
        let current_price = sells[sell_idx].limit_price;
        if current_price > price {
            break;
        }

        // Collect all sells at this price level
        let mut price_level_sells: Vec<&Order> = Vec::new();
        let mut price_level_supply = 0.0;
        while sell_idx < sells.len() && (sells[sell_idx].limit_price - current_price).abs() < 1e-9 {
            if sells[sell_idx].limit_price <= price {
                price_level_sells.push(sells[sell_idx]);
                price_level_supply += sells[sell_idx].quantity;
            }
            sell_idx += 1;
        }

        if price_level_sells.is_empty() {
            continue;
        }

        // Proportional allocation at this price level
        let fill_ratio = if price_level_supply > remaining_supply {
            remaining_supply / price_level_supply
        } else {
            1.0
        };

        for sell in price_level_sells {
            let fill_qty = sell.quantity * fill_ratio;
            if fill_qty > 0.0 {
                remaining_supply -= fill_qty;
                fills.push(Fill {
                    order_id: sell.id,
                    agent_id: sell.agent_id,
                    good,
                    side: Side::Sell,
                    quantity: fill_qty,
                    price,
                });
            }
        }
    }

    MarketClearResult {
        clearing_price: Some(price),
        fills,
    }
}

// === AGENT BUDGET STATE ===

struct AgentBudget {
    #[allow(dead_code)] // Stored for debugging
    agent_id: AgentId,
    currency: f64,

    // track tentative position across iterations
    tentative_buys: f64,  // total purchase cost
    tentative_sells: f64, // total sale revenue
}

impl AgentBudget {
    fn net_flow(&self) -> f64 {
        self.tentative_sells - self.tentative_buys
    }

    fn is_feasible(&self) -> bool {
        self.currency + self.net_flow() >= 0.0
    }
}

// === BUDGET RELAXATION ===

struct RelaxationResult {
    orders_to_remove: Vec<u64>, // order ids
    all_feasible: bool,
}

/// Find orders to remove from agents violating budget constraint
fn compute_relaxation(
    agents: &HashMap<AgentId, AgentBudget>,
    active_orders: &[Order],
    tentative_fills: &[Fill],
) -> RelaxationResult {
    let mut to_remove = Vec::new();

    for (agent_id, budget) in agents {
        if budget.is_feasible() {
            continue;
        }

        let shortfall = -(budget.currency + budget.net_flow());

        // find this agent's buy orders that filled, sorted by limit_price ascending (cut cheapest bids first)
        let mut agent_buys: Vec<_> = tentative_fills
            .iter()
            .filter(|f| f.agent_id == *agent_id && matches!(f.side, Side::Buy))
            .collect();

        agent_buys.sort_by(|a, b| {
            let order_a = active_orders.iter().find(|o| o.id == a.order_id).unwrap();
            let order_b = active_orders.iter().find(|o| o.id == b.order_id).unwrap();
            order_a
                .limit_price
                .partial_cmp(&order_b.limit_price)
                .unwrap()
        });

        // remove orders until feasible
        let mut recovered = 0.0;
        for fill in agent_buys {
            if recovered >= shortfall {
                break;
            }
            to_remove.push(fill.order_id);
            recovered += fill.quantity * fill.price;
        }
    }

    RelaxationResult {
        all_feasible: to_remove.is_empty(),
        orders_to_remove: to_remove,
    }
}

// === MAIN ITERATED AUCTION ===

pub struct MultiMarketResult {
    pub clearing_prices: HashMap<GoodId, Price>,
    pub fills: Vec<Fill>,
    pub iterations: u32,
}

pub fn clear_multi_market(
    goods: &[GoodId],
    mut orders: Vec<Order>,
    initial_budgets: &HashMap<AgentId, f64>,
    max_iterations: u32,
    bias: PriceBias,
) -> MultiMarketResult {
    let mut iteration = 0;

    loop {
        iteration += 1;

        // 1. Clear each market independently
        let mut all_fills = Vec::new();
        let mut clearing_prices = HashMap::new();

        for good in goods {
            let good_orders: Vec<_> = orders.iter().filter(|o| o.good == *good).cloned().collect();

            let result = clear_single_market(*good, &good_orders, bias);

            if let Some(price) = result.clearing_price {
                clearing_prices.insert(*good, price);
            }
            all_fills.extend(result.fills);
        }

        // 2. Compute tentative budgets
        let mut budgets: HashMap<AgentId, AgentBudget> = initial_budgets
            .iter()
            .map(|(id, currency)| {
                (
                    *id,
                    AgentBudget {
                        agent_id: *id,
                        currency: *currency,
                        tentative_buys: 0.0,
                        tentative_sells: 0.0,
                    },
                )
            })
            .collect();

        for fill in &all_fills {
            let budget = budgets.get_mut(&fill.agent_id).unwrap();
            match fill.side {
                Side::Buy => budget.tentative_buys += fill.quantity * fill.price,
                Side::Sell => budget.tentative_sells += fill.quantity * fill.price,
            }
        }

        // 3. Check constraints and relax
        let relaxation = compute_relaxation(&budgets, &orders, &all_fills);

        if relaxation.all_feasible {
            return MultiMarketResult {
                clearing_prices,
                fills: all_fills,
                iterations: iteration,
            };
        }

        if iteration >= max_iterations {
            // return best effort, or could error
            return MultiMarketResult {
                clearing_prices,
                fills: all_fills,
                iterations: iteration,
            };
        }

        // 4. Remove violating orders and iterate
        orders.retain(|o| !relaxation.orders_to_remove.contains(&o.id));
    }
}

// === FILL APPLICATION ===

pub fn apply_fill(pop: &mut Pop, fill: &Fill) {
    match fill.side {
        Side::Buy => {
            pop.currency -= fill.quantity * fill.price;
            *pop.stocks.entry(fill.good).or_insert(0.0) += fill.quantity;
        }
        Side::Sell => {
            pop.currency += fill.quantity * fill.price;
            *pop.stocks.entry(fill.good).or_insert(0.0) -= fill.quantity;
        }
    }
}

/// Apply a fill to a merchant at a specific settlement.
/// Merchants have per-settlement stockpiles, so settlement context is required.
pub fn apply_fill_merchant(merchant: &mut MerchantAgent, settlement: SettlementId, fill: &Fill) {
    // Update currency first (before borrowing stockpile)
    match fill.side {
        Side::Buy => {
            merchant.currency -= fill.quantity * fill.price;
        }
        Side::Sell => {
            merchant.currency += fill.quantity * fill.price;
        }
    }

    // Then update stockpile
    let stockpile = merchant.stockpile_at(settlement);
    match fill.side {
        Side::Buy => {
            stockpile.add(fill.good, fill.quantity);
        }
        Side::Sell => {
            stockpile.remove(fill.good, fill.quantity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buy(id: u64, agent_id: u32, qty: f64, price: f64) -> Order {
        Order {
            id,
            agent_id,
            good: 1,
            side: Side::Buy,
            quantity: qty,
            limit_price: price,
        }
    }

    fn make_sell(id: u64, agent_id: u32, qty: f64, price: f64) -> Order {
        Order {
            id,
            agent_id,
            good: 1,
            side: Side::Sell,
            quantity: qty,
            limit_price: price,
        }
    }

    #[test]
    fn proportional_allocation_at_same_price() {
        // Facility A wants 20 workers at price 10
        // Facility B wants 30 workers at price 10
        // Only 10 workers available at price 5
        // Expected: A gets 4, B gets 6 (proportional to 20:30 = 2:3)

        let mut orders = Vec::new();

        // Facility A (agent_id=1) bids for 20 at price 10
        for i in 0..20 {
            orders.push(make_buy(i, 1, 1.0, 10.0));
        }

        // Facility B (agent_id=2) bids for 30 at price 10
        for i in 20..50 {
            orders.push(make_buy(i, 2, 1.0, 10.0));
        }

        // 10 workers available at price 5
        for i in 100..110 {
            orders.push(make_sell(i, i as u32, 1.0, 5.0));
        }

        let result = clear_single_market(1, &orders, PriceBias::FavorSellers);

        assert!(result.clearing_price.is_some());
        let price = result.clearing_price.unwrap();
        assert!(
            (price - 10.0).abs() < 0.01,
            "clearing price should be 10, got {}",
            price
        );

        // Count fills per agent
        let agent_1_fills: f64 = result
            .fills
            .iter()
            .filter(|f| f.agent_id == 1 && matches!(f.side, Side::Buy))
            .map(|f| f.quantity)
            .sum();

        let agent_2_fills: f64 = result
            .fills
            .iter()
            .filter(|f| f.agent_id == 2 && matches!(f.side, Side::Buy))
            .map(|f| f.quantity)
            .sum();

        // A wants 20, B wants 30, total 50. Only 10 available.
        // A should get 20/50 * 10 = 4
        // B should get 30/50 * 10 = 6
        assert!(
            (agent_1_fills - 4.0).abs() < 0.01,
            "Agent 1 should get 4, got {}",
            agent_1_fills
        );
        assert!(
            (agent_2_fills - 6.0).abs() < 0.01,
            "Agent 2 should get 6, got {}",
            agent_2_fills
        );

        // Total should be 10
        let total_fills: f64 = result
            .fills
            .iter()
            .filter(|f| matches!(f.side, Side::Buy))
            .map(|f| f.quantity)
            .sum();
        assert!(
            (total_fills - 10.0).abs() < 0.01,
            "Total fills should be 10, got {}",
            total_fills
        );
    }
}
