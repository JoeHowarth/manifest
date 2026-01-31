use std::collections::HashMap;

// === CORE TYPES ===

pub type GoodId = u32;
pub type AgentId = u32;
pub type Price = f64;
pub type Quantity = f64;

// === NEEDS & UTILITY ===

pub enum UtilityCurve {
    /// Essentials: high marginal utility until satisfied, then drops
    Subsistence { requirement: f64, steepness: f64 },

    /// Comforts: smooth diminishing returns
    LogDiminishing { scale: f64 },

    /// Luxuries: only kicks in above baseline, convex initially
    LuxuryThreshold { threshold: f64, scale: f64 },

    /// Status goods: relative to neighbors/expectations
    Positional { reference: f64, sensitivity: f64 },
}

impl UtilityCurve {
    pub fn marginal_utility(&self, current_satisfaction: f64) -> f64 {
        match self {
            Self::Subsistence {
                requirement,
                steepness,
            } => {
                let ratio = current_satisfaction / requirement;
                if ratio < 1.0 {
                    steepness * (1.0 - ratio).powi(2)
                } else {
                    0.01 / ratio
                }
            }
            Self::LogDiminishing { scale } => scale / (1.0 + current_satisfaction),
            Self::LuxuryThreshold { threshold, scale } => {
                if current_satisfaction < *threshold {
                    0.0
                } else {
                    scale / (1.0 + current_satisfaction - threshold)
                }
            }
            Self::Positional {
                reference,
                sensitivity,
            } => sensitivity * (reference - current_satisfaction).tanh(),
        }
    }
}

pub struct Need {
    pub id: String,
    pub utility_curve: UtilityCurve,
}

// === CONSUMPTION ===

pub struct ConsumptionResult {
    pub actual: HashMap<GoodId, Quantity>,
    pub desired: HashMap<GoodId, Quantity>,
}

// === AGENTS ===

pub struct PopulationState {
    pub id: AgentId,
    pub currency: f64,
    pub stocks: HashMap<GoodId, Quantity>,
    pub desired_consumption_ema: HashMap<GoodId, Quantity>,
    pub need_satisfaction: HashMap<String, f64>,
    /// Smoothed income used as budget for desire discovery and market purchases.
    /// TODO: Update this after income events (wages, sales) with:
    ///   income_ema = 0.8 * income_ema + 0.2 * income_this_tick
    pub income_ema: f64,
}

pub struct MerchantAgent {
    pub id: AgentId,
    pub currency: f64,
    pub stocks: HashMap<GoodId, Quantity>,
}

impl MerchantAgent {
    pub fn generate_orders(&self, _price_ema: &HashMap<GoodId, Price>) -> Vec<Order> {
        Vec::new()
    }
}

// === ORDERS & FILLS ===

#[derive(Clone)]
pub struct Order {
    pub id: u64,
    pub agent_id: AgentId,
    pub good: GoodId,
    pub side: Side,
    pub quantity: Quantity,
    pub limit_price: Price,
}

#[derive(Clone, Copy)]
pub enum Side {
    Buy,
    Sell,
}

pub struct Fill {
    pub order_id: u64,
    pub agent_id: AgentId,
    pub good: GoodId,
    pub side: Side,
    pub quantity: Quantity,
    pub price: Price,
}
