// Facility type for production

use std::collections::HashMap;

use crate::labor::SkillId;
use crate::types::{FacilityId, MerchantId, SettlementId};

/// What a facility produces
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FacilityType {
    // Primary production (require natural resources)
    Farm,     // FertileLand → grain
    Fishery,  // Fishery → fish
    Sawmill,  // Forest → lumber
    IronMine, // IronOre → iron

    // Secondary production (transform goods)
    Bakery, // grain → bread
    Smithy, // iron → tools
}

/// A production facility at a settlement
#[derive(Debug, Clone)]
pub struct Facility {
    pub id: FacilityId,
    pub facility_type: FacilityType,
    pub settlement: SettlementId,
    pub owner: MerchantId,

    /// Currency available for paying wages
    pub currency: f64,

    /// Current employees by primary skill
    pub workers: HashMap<SkillId, u32>,
}

impl Facility {
    pub fn new(
        id: FacilityId,
        facility_type: FacilityType,
        settlement: SettlementId,
        owner: MerchantId,
    ) -> Self {
        Self {
            id,
            facility_type,
            settlement,
            owner,
            currency: 0.0,
            workers: HashMap::new(),
        }
    }

    pub fn with_currency(mut self, currency: f64) -> Self {
        self.currency = currency;
        self
    }
}
