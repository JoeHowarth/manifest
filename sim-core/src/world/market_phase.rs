use super::*;

impl World {
    pub(super) fn run_market_phase_settlement(
        &mut self,
        settlement_id: SettlementId,
        good_profiles: &[GoodProfile],
        needs: &HashMap<String, crate::needs::Need>,
        merchants: &mut HashMap<MerchantId, MerchantAgent>,
    ) {
        let Some(settlement) = self.settlements.get_mut(&settlement_id) else {
            return;
        };

        if let Some(config) = &self.external_market {
            for (&good, anchor) in &config.anchors {
                let current = settlement
                    .depth_multipliers
                    .get(&good)
                    .copied()
                    .unwrap_or(1.0);
                let local_price = settlement.price_ema.get(&good).copied();
                let new_mult = crate::external::compute_depth_multiplier(
                    current,
                    local_price,
                    anchor.world_price,
                );
                settlement.depth_multipliers.insert(good, new_mult);
            }
        }

        let merchant_ids = crate::determinism::sorted_merchant_ids(
            settlement.owner_facility_counts.keys().copied(),
        );
        let mut extracted_merchants: Vec<(MerchantId, MerchantAgent)> = merchant_ids
            .iter()
            .filter_map(|id| merchants.remove(id).map(|m| (*id, m)))
            .collect();

        let mut pop_refs: Vec<(PopKey, &mut Pop)> = settlement.pops.iter_mut().collect();
        let mut merchant_refs: Vec<&mut MerchantAgent> =
            extracted_merchants.iter_mut().map(|(_, m)| m).collect();

        let _result = run_settlement_tick(
            self.tick,
            settlement_id,
            &mut pop_refs,
            &mut merchant_refs,
            good_profiles,
            needs,
            &mut settlement.price_ema,
            self.external_market.as_ref(),
            Some(&mut self.outside_flow_totals),
            self.subsistence_reservation.as_ref(),
            &settlement.depth_multipliers,
            Some(&settlement.subsistence_queue),
        );

        for (id, merchant) in extracted_merchants {
            merchants.insert(id, merchant);
        }
    }
}
