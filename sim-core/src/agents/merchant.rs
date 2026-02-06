use std::collections::{HashMap, HashSet};

use crate::agents::Stockpile;
use crate::market::{Order, Side};
use crate::types::{FacilityId, GoodId, MerchantId, Price, SettlementId};

// === SUPPLY CURVE CONSTANTS ===

const PRICE_SWEEP_MIN: f64 = 0.6;
const PRICE_SWEEP_MAX: f64 = 1.4;
const PRICE_SWEEP_POINTS: usize = 9;
const TARGET_STOCK_BUFFER: f64 = 2.0; // ticks of production to hold as buffer

/// Quantity supplied as fraction of excess above target.
///
/// Two competing forces:
/// - norm_c: stock / target (>1 = above target, want to sell more)
/// - norm_p: price / EMA (>1 = good price, want to sell more)
///
/// Returns value in [0, 1] representing willingness to sell.
fn qty_supply(norm_p: f64, norm_c: f64) -> f64 {
    // How much above target? (0 = at target, 1 = double target)
    let excess_ratio = (norm_c - 1.0).max(0.0);
    // How attractive is the price? (0 = at EMA, positive = above EMA)
    let price_factor = (norm_p - 1.0).max(-0.3); // floor at -0.3 to always sell some at low prices if overstocked

    // Base willingness from being overstocked, boosted by good prices
    (excess_ratio * (0.5 + 0.5 * price_factor) + 0.1 * price_factor.max(0.0)).clamp(0.0, 1.0)
}

/// A merchant entity that can trade across settlements.
/// Has agency - controlled by player or AI bot.
#[derive(Debug, Clone)]
pub struct MerchantAgent {
    pub id: MerchantId,
    pub currency: f64,
    /// Facilities owned by this merchant (enables stockpiling at those settlements)
    pub facility_ids: HashSet<FacilityId>,
    /// Stockpiles at settlements where merchant owns facilities
    pub stockpiles: HashMap<SettlementId, Stockpile>,
    /// EMA of production rate per good per settlement (for buffer target calculation)
    pub production_ema: HashMap<SettlementId, HashMap<GoodId, f64>>,
}

impl MerchantAgent {
    pub fn new(id: MerchantId) -> Self {
        Self {
            id,
            currency: 10000.0,
            facility_ids: HashSet::new(),
            stockpiles: HashMap::new(),
            production_ema: HashMap::new(),
        }
    }

    pub fn with_currency(mut self, currency: f64) -> Self {
        self.currency = currency;
        self
    }

    /// Check if merchant can stockpile at a settlement (owns a facility there)
    pub fn can_stockpile_at(&self, _settlement: SettlementId) -> bool {
        // TODO: Need to look up facility locations
        // For now, just check if they have any stockpile there
        false
    }

    /// Get or create stockpile at a settlement (caller must verify facility ownership)
    pub fn stockpile_at(&mut self, settlement: SettlementId) -> &mut Stockpile {
        self.stockpiles.entry(settlement).or_default()
    }

    /// Record production output and update the production EMA.
    /// Call this when goods are produced at a facility owned by this merchant.
    pub fn record_production(&mut self, settlement: SettlementId, good: GoodId, quantity: f64) {
        let ema = self
            .production_ema
            .entry(settlement)
            .or_default()
            .entry(good)
            .or_insert(quantity); // Initialize to first observation
        // Blend new production into EMA (α = 0.3 for responsiveness)
        *ema = 0.7 * *ema + 0.3 * quantity;
    }

    /// Get expected production rate for a good at a settlement
    pub fn expected_production(&self, settlement: SettlementId, good: GoodId) -> f64 {
        self.production_ema
            .get(&settlement)
            .and_then(|goods| goods.get(&good))
            .copied()
            .unwrap_or(0.0)
    }

    /// Generate market orders for a settlement.
    ///
    /// Supply curve with two forces:
    /// 1. Stock level: below target → less willing to sell; above target → more willing
    /// 2. Price: above EMA → more willing to sell; below EMA → less willing
    ///
    /// Generates multiple orders across price points (like pop's demand curve).
    pub fn generate_orders(
        &self,
        settlement: SettlementId,
        price_ema: &HashMap<GoodId, Price>,
    ) -> Vec<Order> {
        let mut orders = Vec::new();

        // Get stockpile at this settlement
        let Some(stockpile) = self.stockpiles.get(&settlement) else {
            return orders;
        };

        for (&good, &qty) in &stockpile.goods {
            if qty < 0.01 {
                continue;
            }

            let ema_price = price_ema.get(&good).copied().unwrap_or(1.0);
            // Target buffer = ticks × expected production rate
            // Falls back to 1.0 if no production data yet (so target = 2.0 minimum)
            let production_rate = self.expected_production(settlement, good).max(1.0);
            let target = TARGET_STOCK_BUFFER * production_rate;
            let norm_c = qty / target;

            // Debug: trace merchant state
            eprintln!(
                "[merchant] good={} stock={:.1} prod_ema={:.1} target={:.1} norm_c={:.2}",
                good,
                qty,
                self.expected_production(settlement, good),
                target,
                norm_c
            );

            // Sweep price points and generate supply curve
            for i in 0..PRICE_SWEEP_POINTS {
                let norm_p = PRICE_SWEEP_MIN
                    + (PRICE_SWEEP_MAX - PRICE_SWEEP_MIN) * (i as f64)
                        / ((PRICE_SWEEP_POINTS - 1) as f64);

                let qty_frac = qty_supply(norm_p, norm_c);
                let sell_qty = qty_frac * qty; // fraction of current stock

                if sell_qty > 0.001 {
                    orders.push(Order {
                        id: 0, // assigned later
                        agent_id: self.id.0,
                        good,
                        side: Side::Sell,
                        quantity: sell_qty,
                        limit_price: norm_p * ema_price,
                    });
                }
            }
        }

        orders
    }
}
