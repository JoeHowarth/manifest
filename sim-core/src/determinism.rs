use crate::types::{FacilityKey, MerchantId, PopKey, SettlementId, facility_key_u64, pop_key_u64};

pub fn sorted_settlement_ids<I>(iter: I) -> Vec<SettlementId>
where
    I: IntoIterator<Item = SettlementId>,
{
    let mut ids: Vec<SettlementId> = iter.into_iter().collect();
    ids.sort_by_key(|id| id.0);
    ids
}

pub fn sorted_merchant_ids<I>(iter: I) -> Vec<MerchantId>
where
    I: IntoIterator<Item = MerchantId>,
{
    let mut ids: Vec<MerchantId> = iter.into_iter().collect();
    ids.sort_by_key(|id| id.0);
    ids
}

pub fn sorted_pop_keys<I>(iter: I) -> Vec<PopKey>
where
    I: IntoIterator<Item = PopKey>,
{
    let mut keys: Vec<PopKey> = iter.into_iter().collect();
    keys.sort_by_key(|k| pop_key_u64(*k));
    keys
}

pub fn sorted_facility_keys<I>(iter: I) -> Vec<FacilityKey>
where
    I: IntoIterator<Item = FacilityKey>,
{
    let mut keys: Vec<FacilityKey> = iter.into_iter().collect();
    keys.sort_by_key(|k| facility_key_u64(*k));
    keys
}
