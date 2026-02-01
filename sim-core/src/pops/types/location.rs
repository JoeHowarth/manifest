// Location types for multi-settlement economy

use super::ids::SettlementId;

/// Where stockpiles and agents can be located
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub enum LocationId {
    /// At a settlement (warehouse, market)
    Settlement(SettlementId),
    // Future: Ship(ShipId) for goods in transit
}

impl LocationId {
    pub fn settlement(id: SettlementId) -> Self {
        Self::Settlement(id)
    }
}
