use std::collections::{HashMap, HashSet};

use crate::pops::agents::Stockpile;
use crate::pops::labor::FacilityId;
use crate::pops::market::Order;
use crate::pops::types::{GoodId, MerchantId, Price, SettlementId};

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
}

impl MerchantAgent {
    pub fn new(id: MerchantId) -> Self {
        Self {
            id,
            currency: 10000.0,
            facility_ids: HashSet::new(),
            stockpiles: HashMap::new(),
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

    pub fn generate_orders(&self, _price_ema: &HashMap<GoodId, Price>) -> Vec<Order> {
        Vec::new()
    }
}
