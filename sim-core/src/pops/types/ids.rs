// Core ID types and type aliases

// === TYPE ALIASES ===

pub type GoodId = u32;
pub type AgentId = u32;
pub type Price = f64;
pub type Quantity = f64;

// === NEWTYPE IDS ===

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct SettlementId(pub u32);

impl SettlementId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}
