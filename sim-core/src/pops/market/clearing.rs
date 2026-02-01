use std::collections::HashMap;

use crate::pops::agents::{MerchantAgent, Pop};
use crate::pops::types::{AgentId, GoodId, Price, SettlementId};

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

    // Fill buys (highest bidders first, they all pay clearing price)
    for buy in &buys {
        if buy.limit_price < price || remaining_demand <= 0.0 {
            continue;
        }
        let fill_qty = buy.quantity.min(remaining_demand);
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

    // Fill sells (lowest askers first, they all receive clearing price)
    for sell in &sells {
        if sell.limit_price > price || remaining_supply <= 0.0 {
            continue;
        }
        let fill_qty = sell.quantity.min(remaining_supply);
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
