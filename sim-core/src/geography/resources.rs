// Resource types for primary production

use crate::types::FacilityId;

/// Broad categories of natural resources
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// Arable land - farms, orchards, vineyards, ranches
    Land,
    /// Woodland - sawmills, charcoal burners, hunting
    Forest,
    /// Mineral deposits - mines
    OreDeposit,
    /// Access to water - fisheries, salt works, shipyards
    Coastal,
}

/// Quality affects output multiplier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceQuality {
    Poor,   // 0.5x output
    Normal, // 1.0x output
    Rich,   // 1.5x output
}

impl ResourceQuality {
    pub fn multiplier(&self) -> f64 {
        match self {
            Self::Poor => 0.5,
            Self::Normal => 1.0,
            Self::Rich => 1.5,
        }
    }
}

/// A claimable resource slot at a settlement
#[derive(Debug, Clone)]
pub struct ResourceSlot {
    pub resource_type: ResourceType,
    pub quality: ResourceQuality,
    pub claimed_by: Option<FacilityId>,
}

impl ResourceSlot {
    pub fn new(resource_type: ResourceType, quality: ResourceQuality) -> Self {
        Self {
            resource_type,
            quality,
            claimed_by: None,
        }
    }

    pub fn is_available(&self) -> bool {
        self.claimed_by.is_none()
    }

    pub fn claim(&mut self, facility_id: FacilityId) -> bool {
        if self.claimed_by.is_some() {
            return false;
        }
        self.claimed_by = Some(facility_id);
        true
    }

    pub fn release(&mut self) {
        self.claimed_by = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_multipliers() {
        assert_eq!(ResourceQuality::Poor.multiplier(), 0.5);
        assert_eq!(ResourceQuality::Normal.multiplier(), 1.0);
        assert_eq!(ResourceQuality::Rich.multiplier(), 1.5);
    }

    #[test]
    fn test_slot_claim_release() {
        let mut slot = ResourceSlot::new(ResourceType::Land, ResourceQuality::Normal);
        assert!(slot.is_available());

        let facility = FacilityId::new(1);
        assert!(slot.claim(facility));
        assert!(!slot.is_available());
        assert_eq!(slot.claimed_by, Some(facility));

        // Can't double-claim
        let other = FacilityId::new(2);
        assert!(!slot.claim(other));

        slot.release();
        assert!(slot.is_available());
    }
}
