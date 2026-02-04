use crate::types::{AgentId, GoodId, Price, Quantity};

// === ORDERS & FILLS ===

#[derive(Clone, Debug)]
pub struct Order {
    pub id: u64,
    pub agent_id: AgentId,
    pub good: GoodId,
    pub side: Side,
    pub quantity: Quantity,
    pub limit_price: Price,
}

#[derive(Clone, Copy, Debug)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct Fill {
    pub order_id: u64,
    pub agent_id: AgentId,
    pub good: GoodId,
    pub side: Side,
    pub quantity: Quantity,
    pub price: Price,
}
