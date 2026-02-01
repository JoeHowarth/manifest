use std::collections::HashMap;

use crate::pops::types::{AgentId, GoodId, Price, Quantity};
use crate::pops::market::Order;

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
