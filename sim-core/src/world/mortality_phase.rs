use super::*;

impl World {
    pub(super) fn run_mortality_phase_settlement(&mut self, settlement_id: SettlementId) {
        if self.tick <= self.mortality_grace_ticks {
            return;
        }

        let Some(settlement) = self.settlements.get_mut(&settlement_id) else {
            return;
        };

        let any_food_tracked = settlement
            .pops
            .values()
            .any(|p| p.need_satisfaction.contains_key("food"));
        if !any_food_tracked {
            return;
        }

        let mut rng = self.rng.clone();

        let pop_keys = crate::determinism::sorted_pop_keys(settlement.pops.keys());

        let mut outcomes: Vec<(PopKey, MortalityOutcome, f64)> = Vec::with_capacity(pop_keys.len());
        for pop_key in &pop_keys {
            let Some(pop) = settlement.pops.get(*pop_key) else {
                continue;
            };
            let food_satisfaction = pop.need_satisfaction.get("food").copied().unwrap_or(0.0);
            let outcome = check_mortality(&mut rng, food_satisfaction);
            outcomes.push((*pop_key, outcome, food_satisfaction));
        }

        #[cfg(feature = "instrument")]
        for (pop_key, outcome, food_satisfaction) in &outcomes {
            let outcome_str = match outcome {
                MortalityOutcome::Dies => "dies",
                MortalityOutcome::Grows => "grows",
                MortalityOutcome::Survives => "survives",
            };
            let death_prob = crate::mortality::death_probability(*food_satisfaction);
            let growth_prob = crate::mortality::growth_probability(*food_satisfaction);
            tracing::info!(
                target: "mortality",
                tick = self.tick,
                pop_id = pop_key_u64(*pop_key),
                settlement_id = settlement_id.0,
                food_satisfaction = *food_satisfaction,
                death_prob = death_prob,
                growth_prob = growth_prob,
                outcome = outcome_str,
            );
        }

        let mut dead_pops: Vec<PopKey> = Vec::new();
        let mut children: Vec<Pop> = Vec::new();

        for (pop_key, outcome, _food_satisfaction) in outcomes {
            match outcome {
                MortalityOutcome::Dies => dead_pops.push(pop_key),
                MortalityOutcome::Grows => {
                    if let Some(parent) = settlement.pops.get_mut(pop_key) {
                        let mut child = parent.clone();
                        let child_currency = parent.currency * 0.4;
                        parent.currency -= child_currency;
                        child.currency = child_currency;
                        for (good, qty) in &mut child.stocks {
                            let child_share = *qty * 0.4;
                            if let Some(parent_qty) = parent.stocks.get_mut(good) {
                                *parent_qty -= child_share;
                            }
                            *qty = child_share;
                        }
                        child.employed_at = None;
                        children.push(child);
                    }
                }
                MortalityOutcome::Survives => {}
            }
        }

        for pop_key in dead_pops {
            let Some(pop) = settlement.pops.remove(pop_key) else {
                continue;
            };

            settlement.subsistence_queue.retain(|k| *k != pop_key);

            if let Some(facility_key) = pop.employed_at
                && let Some(facility) = settlement.facilities.get_mut(facility_key)
            {
                for skill in &pop.skills {
                    if let Some(count) = facility.workers.get_mut(skill) {
                        *count = count.saturating_sub(1);
                    }
                }
            }

            use rand::seq::SliceRandom;
            let mut heirs = crate::determinism::sorted_pop_keys(settlement.pops.keys());
            heirs.shuffle(&mut rng);
            heirs.truncate(3);
            let n = heirs.len();
            if n > 0 {
                let share = 1.0 / n as f64;
                for heir_key in heirs {
                    if let Some(heir) = settlement.pops.get_mut(heir_key) {
                        heir.currency += pop.currency * share;
                        for (good, qty) in &pop.stocks {
                            *heir.stocks.entry(*good).or_insert(0.0) += qty * share;
                        }
                    }
                }
            }
        }

        for child in children {
            settlement.pops.insert(child);
        }

        self.rng = rng;
    }
}
