// Settlement type for multi-location economy

use crate::types::{FacilityId, PopId, SettlementId};

use super::resources::{ResourceSlot, ResourceType};

/// A node in the trade network
#[derive(Debug, Clone)]
pub struct Settlement {
    pub id: SettlementId,
    pub name: String,
    pub position: (f64, f64),
    pub pop_ids: Vec<PopId>,
    pub resource_slots: Vec<ResourceSlot>,
}

impl Settlement {
    pub fn new(id: SettlementId, name: impl Into<String>, position: (f64, f64)) -> Self {
        Self {
            id,
            name: name.into(),
            position,
            pop_ids: Vec::new(),
            resource_slots: Vec::new(),
        }
    }

    pub fn with_pops(mut self, pop_ids: Vec<PopId>) -> Self {
        self.pop_ids = pop_ids;
        self
    }

    pub fn with_resources(mut self, slots: Vec<ResourceSlot>) -> Self {
        self.resource_slots = slots;
        self
    }

    /// Find an available slot of the given resource type
    pub fn find_available_slot(&self, resource_type: ResourceType) -> Option<usize> {
        self.resource_slots
            .iter()
            .position(|s| s.resource_type == resource_type && s.is_available())
    }

    /// Claim a resource slot for a facility
    pub fn claim_slot(&mut self, slot_index: usize, facility_id: FacilityId) -> bool {
        if let Some(slot) = self.resource_slots.get_mut(slot_index) {
            slot.claim(facility_id)
        } else {
            false
        }
    }

    /// Release a resource slot (when facility is demolished)
    pub fn release_slot(&mut self, facility_id: FacilityId) {
        for slot in &mut self.resource_slots {
            if slot.claimed_by == Some(facility_id) {
                slot.release();
            }
        }
    }

    /// Get the slot claimed by a facility
    pub fn get_facility_slot(&self, facility_id: FacilityId) -> Option<&ResourceSlot> {
        self.resource_slots
            .iter()
            .find(|s| s.claimed_by == Some(facility_id))
    }
}
