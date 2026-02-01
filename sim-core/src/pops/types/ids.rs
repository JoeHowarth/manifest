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

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct PopId(pub u32);

impl PopId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct MerchantId(pub u32);

impl MerchantId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}
