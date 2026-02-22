// Core ID types and type aliases
#[cfg(test)]
use slotmap::KeyData;
use slotmap::{Key, new_key_type};

// === TYPE ALIASES ===

pub type GoodId = u32;
pub type AgentId = u64;
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

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct FacilityId(pub u32);

impl FacilityId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

// Canonical runtime identities for settlement-local arenas.
new_key_type! { pub struct PopKey; }
new_key_type! { pub struct FacilityKey; }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PopHandle {
    pub settlement: SettlementId,
    pub key: PopKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FacilityHandle {
    pub settlement: SettlementId,
    pub key: FacilityKey,
}

pub fn pop_key_u64(k: PopKey) -> u64 {
    k.data().as_ffi()
}

pub fn facility_key_u64(k: FacilityKey) -> u64 {
    k.data().as_ffi()
}

#[cfg(test)]
pub fn pop_key_from_u64(v: u64) -> PopKey {
    PopKey::from(KeyData::from_ffi(v))
}

#[cfg(test)]
pub fn facility_key_from_u64(v: u64) -> FacilityKey {
    FacilityKey::from(KeyData::from_ffi(v))
}
