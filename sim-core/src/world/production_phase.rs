use super::*;
use crate::agents::merchant::PRODUCTION_EMA_RETAIN;

impl World {
    pub(super) fn run_production_phase_settlement(
        &mut self,
        settlement_id: SettlementId,
        recipes: &[Recipe],
        merchants: &mut HashMap<MerchantId, MerchantAgent>,
    ) {
        let Some(settlement) = self.settlements.get_mut(&settlement_id) else {
            return;
        };

        let mut production_totals: HashMap<(MerchantId, GoodId), f64> = HashMap::new();
        let facility_keys = crate::determinism::sorted_facility_keys(settlement.facilities.keys());

        for facility_key in facility_keys {
            let (owner_id, quality_multiplier) = {
                let Some(facility) = settlement.facilities.get(facility_key) else {
                    continue;
                };
                let quality = settlement
                    .info
                    .get_facility_slot(facility_key)
                    .map(|slot| slot.quality.multiplier())
                    .unwrap_or(1.0);
                (facility.owner, quality)
            };

            let Some(merchant) = merchants.get_mut(&owner_id) else {
                continue;
            };
            let stockpile = merchant
                .stockpiles
                .entry(settlement_id)
                .or_insert_with(Stockpile::new);

            let Some(facility) = settlement.facilities.get(facility_key) else {
                continue;
            };
            let allocation = allocate_recipes(facility_key, facility, recipes, stockpile);

            let stockpile = merchant
                .stockpiles
                .get_mut(&settlement_id)
                .expect("stockpile must exist");
            let result = execute_production(&allocation, recipes, stockpile, quality_multiplier);

            for (&good_id, &qty) in &result.outputs_produced {
                if qty > 0.0 {
                    *production_totals.entry((owner_id, good_id)).or_insert(0.0) += qty;
                }
            }
        }

        // Record production EMA for goods that were produced this tick.
        for (&(merchant_id, good_id), &total_qty) in &production_totals {
            if let Some(merchant) = merchants.get_mut(&merchant_id) {
                merchant.record_production(settlement_id, good_id, total_qty);
            }
        }

        // Decay production EMA for goods NOT produced this tick.
        // 0.7 * ema + 0.3 * 0.0 = 0.7 * ema
        for merchant in merchants.values_mut() {
            if let Some(goods_ema) = merchant.production_ema.get_mut(&settlement_id) {
                goods_ema.retain(|good, ema| {
                    if !production_totals.contains_key(&(merchant.id, *good)) {
                        *ema *= PRODUCTION_EMA_RETAIN;
                    }
                    *ema > 0.001 // prune near-zero entries
                });
            }
        }
    }
}
