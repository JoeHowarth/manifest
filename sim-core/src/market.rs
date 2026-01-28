use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tsify_next::Tsify;

use crate::types::{EntityId, Good};

// ============================================================================
// Labor Market - Where wages emerge from supply and demand
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct LaborMarket {
    pub supply: f32, // Workers available
    pub demand: f32, // Workers wanted by facilities
    pub wage: f32,   // Current clearing wage
}

impl Default for LaborMarket {
    fn default() -> Self {
        Self {
            supply: 0.0,
            demand: 0.0,
            wage: 10.0, // Base wage
        }
    }
}

// ============================================================================
// Inventory - Goods held by an entity
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub items: HashMap<Good, f32>,
}

impl Inventory {
    pub fn add(&mut self, good: Good, amount: f32) {
        *self.items.entry(good).or_insert(0.0) += amount;
    }

    pub fn remove(&mut self, good: Good, amount: f32) -> f32 {
        let current = self.items.entry(good).or_insert(0.0);
        let removed = amount.min(*current);
        *current -= removed;
        removed
    }

    pub fn get(&self, good: Good) -> f32 {
        self.items.get(&good).copied().unwrap_or(0.0)
    }
}

// ============================================================================
// Auction Types - Unified bid/ask mechanism for all markets (v2)
// ============================================================================

/// A bid to buy a good (or labor) at up to a maximum price
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bid {
    pub buyer: EntityId,
    pub good: Good,
    pub quantity: f32,
    pub max_price: f32, // Won't pay more than this
}

/// An ask to sell a good (or labor) at at least a minimum price
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ask {
    pub seller: EntityId,
    pub good: Good,
    pub quantity: f32,
    pub min_price: f32, // Won't accept less than this
}

/// A completed transaction from auction clearing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub seller: EntityId,
    pub buyer: EntityId,
    pub good: Good,
    pub quantity: f32,
    pub price: f32, // The clearing price (midpoint of bid/ask)
}

/// Market price tracking for a single good at a settlement
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct MarketPrice {
    pub good: Good,
    pub last_price: f32,
    pub volume_weighted_avg: f32,
    pub last_traded_quantity: f32,
}

impl MarketPrice {
    pub fn new(good: Good, initial_price: f32) -> Self {
        Self {
            good,
            last_price: initial_price,
            volume_weighted_avg: initial_price,
            last_traded_quantity: 0.0,
        }
    }

    /// Update price tracking after a transaction
    ///
    /// Note: Spec v2 describes hysteresis with threshold=1.0 and max_change=5.0 to prevent
    /// oscillation. Current implementation uses simple EMA which provides smoothing but not
    /// the exact thresholding behavior. This is simpler and sufficient for now.
    pub fn update(&mut self, price: f32, quantity: f32) {
        self.last_price = price;
        self.last_traded_quantity = quantity;
        // Exponential moving average with 30% weight to new data (spec: rate=0.3)
        self.volume_weighted_avg = self.volume_weighted_avg * 0.7 + price * 0.3;
    }
}

/// Get default initial price for a good
pub fn default_price(good: Good) -> f32 {
    match good {
        Good::Grain => 15.0,
        Good::Fish => 15.0,
        Good::Flour => 18.0,
        Good::Provisions => 20.0, // Anchored to subsistence wage
        Good::Labor => 20.0,      // Subsistence wage
    }
}

// ============================================================================
// Auction Clearing - Uniform price auction
// ============================================================================

/// Clear a market using uniform clearing price auction.
///
/// Algorithm:
/// 1. Sort asks ascending, bids descending by price
/// 2. Find clearing price where supply meets demand
/// 3. ALL trades execute at that single price
///
/// This prevents overpaying/underpaying - everyone gets the market price.
pub fn clear_market(bids: &mut Vec<Bid>, asks: &mut Vec<Ask>) -> Vec<Transaction> {
    if bids.is_empty() || asks.is_empty() {
        return vec![];
    }

    // Sort asks by min_price ascending (cheapest first)
    asks.sort_by(|a, b| {
        a.min_price
            .partial_cmp(&b.min_price)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Sort bids by max_price descending (highest bidder first)
    bids.sort_by(|a, b| {
        b.max_price
            .partial_cmp(&a.max_price)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Phase 1: Find clearing price and quantity
    // Walk through demand curve (bids) and supply curve (asks)
    // Stop when marginal bid < marginal ask
    let mut bid_idx = 0;
    let mut ask_idx = 0;
    let mut bid_qty_consumed = 0.0;
    let mut ask_qty_consumed = 0.0;
    let mut clearing_quantity = 0.0;
    let mut marginal_bid_price = bids[0].max_price;
    let mut marginal_ask_price = asks[0].min_price;

    while bid_idx < bids.len() && ask_idx < asks.len() {
        let bid = &bids[bid_idx];
        let ask = &asks[ask_idx];

        // No more trades possible if bid < ask
        if bid.max_price < ask.min_price {
            break;
        }

        marginal_bid_price = bid.max_price;
        marginal_ask_price = ask.min_price;

        // Match as much as possible at this level
        let bid_avail = bid.quantity - bid_qty_consumed;
        let ask_avail = ask.quantity - ask_qty_consumed;
        let matched = bid_avail.min(ask_avail);

        clearing_quantity += matched;
        bid_qty_consumed += matched;
        ask_qty_consumed += matched;

        // Move to next order if exhausted
        if bid_qty_consumed >= bid.quantity - 0.001 {
            bid_idx += 1;
            bid_qty_consumed = 0.0;
        }
        if ask_qty_consumed >= ask.quantity - 0.001 {
            ask_idx += 1;
            ask_qty_consumed = 0.0;
        }
    }

    if clearing_quantity <= 0.0 {
        return vec![];
    }

    // Clearing price = midpoint of where curves meet
    // marginal_bid_price is the lowest bid that traded
    // marginal_ask_price is the highest ask that traded
    // Price should be in [marginal_ask_price, marginal_bid_price]
    let clearing_price = (marginal_bid_price + marginal_ask_price) / 2.0;

    // Phase 2: Execute trades at uniform clearing price
    // All bids with max_price >= marginal_bid_price trade
    // All asks with min_price <= marginal_ask_price trade
    let mut transactions = Vec::new();
    let mut remaining = clearing_quantity;
    let mut bid_remaining: Vec<f32> = bids.iter().map(|b| b.quantity).collect();
    let mut ask_remaining: Vec<f32> = asks.iter().map(|a| a.quantity).collect();
    bid_idx = 0;
    ask_idx = 0;

    while remaining > 0.001 && bid_idx < bids.len() && ask_idx < asks.len() {
        let bid = &bids[bid_idx];
        let ask = &asks[ask_idx];

        // Skip bids below the marginal (they didn't participate in clearing)
        if bid.max_price < marginal_bid_price - 0.001 {
            bid_idx += 1;
            continue;
        }
        // Skip asks above the marginal
        if ask.min_price > marginal_ask_price + 0.001 {
            ask_idx += 1;
            continue;
        }

        let quantity = bid_remaining[bid_idx]
            .min(ask_remaining[ask_idx])
            .min(remaining);

        if quantity > 0.001 {
            transactions.push(Transaction {
                seller: ask.seller,
                buyer: bid.buyer,
                good: bid.good,
                quantity,
                price: clearing_price,
            });

            bid_remaining[bid_idx] -= quantity;
            ask_remaining[ask_idx] -= quantity;
            remaining -= quantity;
        }

        if bid_remaining[bid_idx] <= 0.001 {
            bid_idx += 1;
        }
        if ask_remaining[ask_idx] <= 0.001 {
            ask_idx += 1;
        }
    }

    transactions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SettlementId;
    use slotmap::KeyData;

    fn dummy_settlement_id() -> SettlementId {
        SettlementId::from(KeyData::from_ffi(1))
    }

    fn dummy_org_id() -> crate::types::OrgId {
        crate::types::OrgId::from(KeyData::from_ffi(2))
    }

    #[test]
    fn test_clear_market_simple_match() {
        let seller = EntityId::from_population(dummy_settlement_id());
        let buyer = EntityId::from_org(dummy_org_id());

        let mut asks = vec![Ask {
            seller,
            good: Good::Labor,
            quantity: 100.0,
            min_price: 20.0,
        }];

        let mut bids = vec![Bid {
            buyer,
            good: Good::Labor,
            quantity: 50.0,
            max_price: 30.0,
        }];

        let txns = clear_market(&mut bids, &mut asks);

        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].quantity, 50.0);
        assert_eq!(txns[0].price, 25.0); // (20 + 30) / 2
    }

    #[test]
    fn test_clear_market_no_match() {
        let seller = EntityId::from_population(dummy_settlement_id());
        let buyer = EntityId::from_org(dummy_org_id());

        let mut asks = vec![Ask {
            seller,
            good: Good::Labor,
            quantity: 100.0,
            min_price: 50.0, // Asking too much
        }];

        let mut bids = vec![Bid {
            buyer,
            good: Good::Labor,
            quantity: 50.0,
            max_price: 30.0, // Not willing to pay enough
        }];

        let txns = clear_market(&mut bids, &mut asks);

        assert_eq!(txns.len(), 0);
    }

    #[test]
    fn test_clear_market_multiple_matches() {
        let seller = EntityId::from_population(dummy_settlement_id());
        let buyer = EntityId::from_org(dummy_org_id());

        // Multiple asks at different prices
        let mut asks = vec![
            Ask {
                seller,
                good: Good::Labor,
                quantity: 30.0,
                min_price: 20.0, // Cheapest
            },
            Ask {
                seller,
                good: Good::Labor,
                quantity: 30.0,
                min_price: 25.0, // Mid
            },
            Ask {
                seller,
                good: Good::Labor,
                quantity: 30.0,
                min_price: 35.0, // Expensive
            },
        ];

        let mut bids = vec![Bid {
            buyer,
            good: Good::Labor,
            quantity: 50.0,
            max_price: 30.0,
        }];

        let txns = clear_market(&mut bids, &mut asks);

        // With uniform clearing price:
        // - Bid 50@30 matches Ask 30@20 and Ask 20@25 (35 is too expensive)
        // - Clearing quantity: 50 (30 + 20)
        // - Marginal bid: 30, Marginal ask: 25
        // - Clearing price: (30 + 25) / 2 = 27.5
        // - All trades at 27.5
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].quantity, 30.0);
        assert_eq!(txns[0].price, 27.5); // Uniform clearing price
        assert_eq!(txns[1].quantity, 20.0);
        assert_eq!(txns[1].price, 27.5); // Uniform clearing price
    }
}
