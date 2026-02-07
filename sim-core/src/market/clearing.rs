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
    Neutral,     // midpoint-oriented tie break at max volume
}

pub struct MarketClearResult {
    pub clearing_price: Option<Price>, // None if no trade
    pub fills: Vec<Fill>,
}

/// Clear one good's market via call auction.
///
/// If `buyer_budgets` is provided, each buyer's effective demand is capped by what they
/// can afford at each price point: `min(order.quantity, budget / price)`.
///
/// If `seller_inventories` is provided, each seller's effective supply is capped by
/// their actual inventory of the good being traded.
///
/// This ensures the volume-maximizing price accounts for both budget and inventory constraints.
pub fn clear_single_market(
    good: GoodId,
    orders: &[Order], // filtered to this good
    buyer_budgets: Option<&HashMap<AgentId, f64>>,
    seller_inventories: Option<&HashMap<AgentId, f64>>,
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
    // Keep canonical ascending order for deterministic tie handling.
    price_points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    price_points.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let tie_midpoint = match (price_points.first(), price_points.last()) {
        (Some(lo), Some(hi)) => (lo + hi) * 0.5,
        _ => 0.0,
    };

    for price in price_points {
        // Demand at this price: aggregate by agent first, then cap each agent's total
        // (an agent with multiple orders can't exceed their budget)
        let demand: f64 = if let Some(budgets) = buyer_budgets {
            // Sum each agent's total quantity at this price
            let mut agent_qty: HashMap<AgentId, f64> = HashMap::new();
            for order in buys.iter().filter(|o| o.limit_price >= price) {
                *agent_qty.entry(order.agent_id).or_insert(0.0) += order.quantity;
            }
            // Cap each agent's total by their budget
            agent_qty
                .iter()
                .map(|(agent_id, &total_qty)| {
                    let budget = budgets.get(agent_id).copied().unwrap_or(f64::MAX);
                    total_qty.min(budget / price)
                })
                .sum()
        } else {
            buys.iter()
                .filter(|o| o.limit_price >= price)
                .map(|o| o.quantity)
                .sum()
        };
        // Supply at this price: aggregate by agent, cap by inventory
        // (a seller can't sell more than they have)
        let supply: f64 = if let Some(inventories) = seller_inventories {
            let mut agent_qty: HashMap<AgentId, f64> = HashMap::new();
            for order in sells.iter().filter(|o| o.limit_price <= price) {
                *agent_qty.entry(order.agent_id).or_insert(0.0) += order.quantity;
            }
            // Cap each agent's total by their inventory
            agent_qty
                .iter()
                .map(|(agent_id, &total_qty)| {
                    let inventory = inventories.get(agent_id).copied().unwrap_or(0.0);
                    total_qty.min(inventory)
                })
                .sum()
        } else {
            sells
                .iter()
                .filter(|o| o.limit_price <= price)
                .map(|o| o.quantity)
                .sum()
        };

        let volume = demand.min(supply);
        if volume > max_volume + 1e-12 {
            max_volume = volume;
            clearing_price = Some(price);
        } else if max_volume > 1e-12 && (volume - max_volume).abs() <= 1e-12 {
            let current = clearing_price.unwrap_or(price);
            let picked = match bias {
                PriceBias::FavorSellers => current.max(price),
                PriceBias::FavorBuyers => current.min(price),
                // In ties, pick the candidate closest to the midpoint of the
                // feasible price grid to avoid systematic drift to one edge.
                PriceBias::Neutral => {
                    if (price - tie_midpoint).abs() < (current - tie_midpoint).abs() {
                        price
                    } else {
                        current
                    }
                }
            };
            clearing_price = Some(picked);
        }
    }

    let Some(price) = clearing_price else {
        return MarketClearResult {
            clearing_price: None,
            fills: Vec::new(),
        };
    };

    // Compute each buyer's max fill at the clearing price (budget-capped total)
    let mut agent_max_fill: HashMap<AgentId, f64> = HashMap::new();
    for order in buys.iter().filter(|o| o.limit_price >= price) {
        *agent_max_fill.entry(order.agent_id).or_insert(0.0) += order.quantity;
    }
    if let Some(budgets) = buyer_budgets {
        for (agent_id, qty) in agent_max_fill.iter_mut() {
            let budget = budgets.get(agent_id).copied().unwrap_or(f64::MAX);
            *qty = (*qty).min(budget / price);
        }
    }

    // Compute each seller's max fill at the clearing price (inventory-capped total)
    let mut seller_max_fill: HashMap<AgentId, f64> = HashMap::new();
    for order in sells.iter().filter(|o| o.limit_price <= price) {
        *seller_max_fill.entry(order.agent_id).or_insert(0.0) += order.quantity;
    }
    if let Some(inventories) = seller_inventories {
        for (agent_id, qty) in seller_max_fill.iter_mut() {
            let inventory = inventories.get(agent_id).copied().unwrap_or(0.0);
            *qty = (*qty).min(inventory);
        }
    }

    // Total budget-constrained demand
    let budget_constrained_demand: f64 = agent_max_fill.values().sum();

    // Total inventory-constrained supply
    let inventory_constrained_supply: f64 = seller_max_fill.values().sum();

    // The actual volume is min of constrained demand and constrained supply
    let actual_volume = budget_constrained_demand.min(inventory_constrained_supply);

    // Generate fills at clearing price
    let mut fills = Vec::new();
    let mut remaining_demand = actual_volume;
    let mut remaining_supply = actual_volume;

    // Track how much each agent has been filled (for budget enforcement)
    let mut agent_filled: HashMap<AgentId, f64> = HashMap::new();

    // Fill buys with proportional allocation at same price levels
    // Process from highest limit price to lowest (most aggressive bidders first)
    let mut buy_idx = 0;
    while buy_idx < buys.len() && remaining_demand > 0.0 {
        let current_price = buys[buy_idx].limit_price;
        if current_price < price {
            break;
        }

        // Collect all buys at this price level, with budget-constrained quantities
        let mut price_level_buys: Vec<(&Order, f64)> = Vec::new(); // (order, effective_qty)
        let mut price_level_demand = 0.0;
        while buy_idx < buys.len() && (buys[buy_idx].limit_price - current_price).abs() < 1e-9 {
            if buys[buy_idx].limit_price >= price {
                let order = buys[buy_idx];
                // Effective qty is limited by agent's remaining budget capacity
                let agent_cap = agent_max_fill.get(&order.agent_id).copied().unwrap_or(0.0);
                let already_filled = agent_filled.get(&order.agent_id).copied().unwrap_or(0.0);
                let remaining_cap = (agent_cap - already_filled).max(0.0);
                let effective_qty = order.quantity.min(remaining_cap);

                if effective_qty > 0.0 {
                    price_level_buys.push((order, effective_qty));
                    price_level_demand += effective_qty;
                }
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

        for (buy, effective_qty) in price_level_buys {
            let fill_qty = effective_qty * fill_ratio;
            if fill_qty > 0.0 {
                remaining_demand -= fill_qty;
                *agent_filled.entry(buy.agent_id).or_insert(0.0) += fill_qty;
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

    // Track how much each seller has filled (for inventory enforcement)
    let mut seller_filled: HashMap<AgentId, f64> = HashMap::new();

    // Fill sells with proportional allocation at same price levels
    // Process from lowest limit price to highest (most aggressive asks first)
    let mut sell_idx = 0;
    while sell_idx < sells.len() && remaining_supply > 0.0 {
        let current_price = sells[sell_idx].limit_price;
        if current_price > price {
            break;
        }

        // Collect all sells at this price level, with inventory-constrained quantities
        let mut price_level_sells: Vec<(&Order, f64)> = Vec::new(); // (order, effective_qty)
        let mut price_level_supply = 0.0;
        while sell_idx < sells.len() && (sells[sell_idx].limit_price - current_price).abs() < 1e-9 {
            if sells[sell_idx].limit_price <= price {
                let order = sells[sell_idx];
                // Effective qty is limited by seller's remaining inventory capacity
                let seller_cap = seller_max_fill.get(&order.agent_id).copied().unwrap_or(0.0);
                let already_filled = seller_filled.get(&order.agent_id).copied().unwrap_or(0.0);
                let remaining_cap = (seller_cap - already_filled).max(0.0);
                let effective_qty = order.quantity.min(remaining_cap);

                if effective_qty > 0.0 {
                    price_level_sells.push((order, effective_qty));
                    price_level_supply += effective_qty;
                }
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

        for (sell, effective_qty) in price_level_sells {
            let fill_qty = effective_qty * fill_ratio;
            if fill_qty > 0.0 {
                remaining_supply -= fill_qty;
                *seller_filled.entry(sell.agent_id).or_insert(0.0) += fill_qty;
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

#[derive(Debug, Clone)]
pub struct MultiMarketResult {
    pub clearing_prices: HashMap<GoodId, Price>,
    pub fills: Vec<Fill>,
    pub iterations: u32,
}

pub fn clear_multi_market(
    goods: &[GoodId],
    mut orders: Vec<Order>,
    initial_budgets: &HashMap<AgentId, f64>,
    seller_inventories: Option<&HashMap<AgentId, HashMap<GoodId, f64>>>,
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

            // Extract per-agent inventory for this specific good
            let good_inventories: Option<HashMap<AgentId, f64>> = seller_inventories.map(|invs| {
                invs.iter()
                    .map(|(&agent_id, goods_map)| {
                        (agent_id, goods_map.get(good).copied().unwrap_or(0.0))
                    })
                    .collect()
            });

            // Pass budgets and inventories to clear_single_market for constraint-aware price discovery
            let result = clear_single_market(
                *good,
                &good_orders,
                Some(initial_budgets),
                good_inventories.as_ref(),
                bias,
            );

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

        let result = clear_single_market(1, &orders, None, None, PriceBias::FavorSellers);

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

    #[test]
    fn budget_constrains_quantity_not_price() {
        // Buyer wants 10 units at price 1.0, but only has budget of 5.0
        // Seller offers 100 units at price 1.0
        // Expected: Clears at price 1.0 (not higher), buyer gets 5 units (not 10)

        let orders = vec![
            make_buy(1, 1, 10.0, 1.0),   // agent 1 wants 10 @ 1.0
            make_sell(2, 2, 100.0, 1.0), // agent 2 offers 100 @ 1.0
        ];

        let mut budgets = HashMap::new();
        budgets.insert(1, 5.0); // agent 1 can only spend 5.0
        budgets.insert(2, 1000.0); // seller has plenty

        let result = clear_single_market(1, &orders, Some(&budgets), None, PriceBias::FavorSellers);

        assert!(result.clearing_price.is_some());
        let price = result.clearing_price.unwrap();
        assert!(
            (price - 1.0).abs() < 0.01,
            "clearing price should be 1.0, got {}",
            price
        );

        // Buyer should get budget/price = 5.0/1.0 = 5 units
        let buyer_fills: f64 = result
            .fills
            .iter()
            .filter(|f| f.agent_id == 1 && matches!(f.side, Side::Buy))
            .map(|f| f.quantity)
            .sum();

        assert!(
            (buyer_fills - 5.0).abs() < 0.01,
            "Buyer should get 5 units (budget/price), got {}",
            buyer_fills
        );
    }

    #[test]
    fn budget_constraint_finds_lower_clearing_price() {
        // This tests the death spiral fix scenario:
        // - Many buyers want goods at various prices (1.2 to 2.8)
        // - At high prices (2.8), buyers can afford small quantities → low volume
        // - At low prices (1.2), buyers can afford more → higher volume
        // Budget-aware clearing should find the lower price that maximizes volume.

        let mut orders = Vec::new();

        // 10 buyers, each with budget 1.0
        // Each generates orders at prices 1.2, 1.6, 2.0, 2.4, 2.8
        for buyer_id in 1..=10u32 {
            orders.push(make_buy(buyer_id as u64 * 10 + 1, buyer_id, 2.0, 1.2)); // want 2 @ 1.2
            orders.push(make_buy(buyer_id as u64 * 10 + 2, buyer_id, 1.5, 1.6));
            orders.push(make_buy(buyer_id as u64 * 10 + 3, buyer_id, 1.0, 2.0));
            orders.push(make_buy(buyer_id as u64 * 10 + 4, buyer_id, 0.5, 2.4));
            orders.push(make_buy(buyer_id as u64 * 10 + 5, buyer_id, 0.1, 2.8)); // tiny @ 2.8
        }

        // Seller offers 100 units across price ladder
        for i in 0..5 {
            let price = 1.2 + i as f64 * 0.4; // 1.2, 1.6, 2.0, 2.4, 2.8
            orders.push(make_sell(200 + i, 100, 20.0, price));
        }

        let mut budgets = HashMap::new();
        for buyer_id in 1..=10u32 {
            budgets.insert(buyer_id, 1.0); // each buyer has budget 1.0
        }
        budgets.insert(100, 10000.0); // seller

        let result = clear_single_market(1, &orders, Some(&budgets), None, PriceBias::FavorSellers);

        assert!(result.clearing_price.is_some());
        let price = result.clearing_price.unwrap();

        // At price 1.2: each buyer can afford 1.0/1.2 = 0.83 units → 8.3 total demand
        // At price 2.8: each buyer can afford 1.0/2.8 = 0.36 units, but order is only 0.1 → 1.0 total
        // Volume at 1.2 should be higher, so clearing price should be 1.2

        println!("Clearing price: {}", price);
        println!("Total fills: {:?}", result.fills.len());

        let total_buy_volume: f64 = result
            .fills
            .iter()
            .filter(|f| matches!(f.side, Side::Buy))
            .map(|f| f.quantity)
            .sum();

        println!("Total buy volume: {}", total_buy_volume);

        // With budget-aware clearing, we should get more volume at a lower price
        // The exact price depends on the supply/demand curves, but it should be lower than 2.8
        assert!(
            price < 2.0,
            "Budget-aware clearing should find a lower price (got {})",
            price
        );
        assert!(
            total_buy_volume > 5.0,
            "Should clear more than 5 units (got {})",
            total_buy_volume
        );
    }

    #[test]
    fn multi_order_agent_respects_total_budget() {
        // Agent 1 has budget 1.0, places orders totaling 3.5 units across 3 price levels
        // At clearing price 1.0, agent can afford 1.0 units total (not 3.5)
        // This tests per-AGENT budget cap, not per-ORDER cap

        let orders = vec![
            make_buy(1, 1, 2.0, 1.0),    // agent 1 wants 2 @ limit 1.0
            make_buy(2, 1, 1.0, 1.5),    // agent 1 wants 1 @ limit 1.5
            make_buy(3, 1, 0.5, 2.0),    // agent 1 wants 0.5 @ limit 2.0
            make_sell(10, 2, 10.0, 1.0), // seller offers 10 @ 1.0
        ];

        let mut budgets = HashMap::new();
        budgets.insert(1, 1.0); // agent 1 has budget 1.0
        budgets.insert(2, 1000.0);

        let result = clear_single_market(1, &orders, Some(&budgets), None, PriceBias::FavorSellers);

        assert!(result.clearing_price.is_some());
        let price = result.clearing_price.unwrap();

        // Should clear at 1.0 - all 3 orders qualify (limits >= 1.0), supply is 10
        // Demand is capped at 1.0 (agent's budget/price), volume = min(1.0, 10) = 1.0
        assert!(
            (price - 1.0).abs() < 0.01,
            "Should clear at 1.0, got {}",
            price
        );

        // Agent 1 should get exactly 1.0 units total (budget cap), not 3.5
        let agent_1_fills: f64 = result
            .fills
            .iter()
            .filter(|f| f.agent_id == 1 && matches!(f.side, Side::Buy))
            .map(|f| f.quantity)
            .sum();

        assert!(
            (agent_1_fills - 1.0).abs() < 0.01,
            "Agent 1 should get 1.0 units (budget cap), got {}",
            agent_1_fills
        );

        // Verify total cost doesn't exceed budget
        let agent_1_cost: f64 = result
            .fills
            .iter()
            .filter(|f| f.agent_id == 1 && matches!(f.side, Side::Buy))
            .map(|f| f.quantity * f.price)
            .sum();

        assert!(
            agent_1_cost <= 1.0 + 0.01,
            "Agent 1 cost {} should not exceed budget 1.0",
            agent_1_cost
        );
    }

    #[test]
    fn inventory_constrains_seller_quantity() {
        // Seller offers 100 units at price 1.0, but only has 30 in inventory
        // Buyer wants 50 units at price 1.0, has plenty of budget
        // Expected: Clears at price 1.0, but seller only sells 30 (inventory cap)

        let orders = vec![
            make_buy(1, 1, 50.0, 1.0),   // agent 1 wants 50 @ 1.0
            make_sell(2, 2, 100.0, 1.0), // agent 2 offers 100 @ 1.0
        ];

        let mut budgets = HashMap::new();
        budgets.insert(1, 1000.0); // buyer has plenty of budget
        budgets.insert(2, 1000.0);

        let mut inventories = HashMap::new();
        inventories.insert(2, 30.0); // seller only has 30 units in stock

        let result = clear_single_market(
            1,
            &orders,
            Some(&budgets),
            Some(&inventories),
            PriceBias::FavorSellers,
        );

        assert!(result.clearing_price.is_some());
        let price = result.clearing_price.unwrap();
        assert!(
            (price - 1.0).abs() < 0.01,
            "clearing price should be 1.0, got {}",
            price
        );

        // Seller should only sell 30 units (inventory cap), not 100
        let seller_fills: f64 = result
            .fills
            .iter()
            .filter(|f| f.agent_id == 2 && matches!(f.side, Side::Sell))
            .map(|f| f.quantity)
            .sum();

        assert!(
            (seller_fills - 30.0).abs() < 0.01,
            "Seller should sell 30 units (inventory cap), got {}",
            seller_fills
        );

        // Buyer gets same amount
        let buyer_fills: f64 = result
            .fills
            .iter()
            .filter(|f| f.agent_id == 1 && matches!(f.side, Side::Buy))
            .map(|f| f.quantity)
            .sum();

        assert!(
            (buyer_fills - 30.0).abs() < 0.01,
            "Buyer should get 30 units (limited by seller inventory), got {}",
            buyer_fills
        );
    }

    #[test]
    fn multi_order_seller_respects_total_inventory() {
        // Seller has 50 units inventory, places orders totaling 80 units across 2 price levels
        // Should only be able to sell 50 total

        let orders = vec![
            make_buy(1, 1, 100.0, 2.0),  // buyer wants 100 @ up to 2.0
            make_sell(10, 2, 50.0, 1.0), // seller offers 50 @ 1.0
            make_sell(11, 2, 30.0, 1.5), // seller offers 30 more @ 1.5 (same seller!)
        ];

        let mut budgets = HashMap::new();
        budgets.insert(1, 10000.0);
        budgets.insert(2, 10000.0);

        let mut inventories = HashMap::new();
        inventories.insert(2, 50.0); // seller only has 50 total

        let result = clear_single_market(
            1,
            &orders,
            Some(&budgets),
            Some(&inventories),
            PriceBias::FavorSellers,
        );

        assert!(result.clearing_price.is_some());

        // Seller's total fills should be capped at 50 (their inventory)
        let seller_fills: f64 = result
            .fills
            .iter()
            .filter(|f| f.agent_id == 2 && matches!(f.side, Side::Sell))
            .map(|f| f.quantity)
            .sum();

        assert!(
            (seller_fills - 50.0).abs() < 0.01,
            "Seller should sell 50 units (inventory cap), got {}",
            seller_fills
        );
    }
}
