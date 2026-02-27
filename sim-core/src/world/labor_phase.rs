use super::*;

impl World {
    fn gather_labor_skills(settlement: &SettlementState, recipes: &[Recipe]) -> Vec<SkillDef> {
        let mut skill_ids: HashSet<SkillId> = settlement.wage_ema.keys().copied().collect();

        for pop in settlement.pops.values() {
            skill_ids.extend(pop.skills.iter().copied());
        }

        for facility in settlement.facilities.values() {
            for recipe_id in &facility.recipe_priorities {
                if let Some(recipe) = recipes.iter().find(|r| r.id == *recipe_id) {
                    skill_ids.extend(recipe.workers.keys().copied());
                }
            }
        }

        let mut ordered_skill_ids: Vec<SkillId> = skill_ids.into_iter().collect();
        ordered_skill_ids.sort_by_key(|s| s.0);

        ordered_skill_ids
            .into_iter()
            .map(|id| SkillDef {
                id,
                name: String::new(),
                parent: None,
            })
            .collect()
    }

    /// Compute per-skill MVPs across all recipes a facility can run.
    ///
    /// For each recipe, the per-worker value is `output_value / total_workers`.
    /// Each skill gets the best (highest) per-worker value across all recipes
    /// that use it. This lets facilities bid differently for high-value vs
    /// low-value skills and ensures secondary recipes' labor needs are visible
    /// to the labor market.
    fn facility_skill_mvps(
        settlement: &SettlementState,
        facility: &Facility,
        recipes: &[Recipe],
    ) -> HashMap<SkillId, Price> {
        let mut skill_mvps: HashMap<SkillId, Price> = HashMap::new();

        for recipe_id in &facility.recipe_priorities {
            let Some(recipe) = recipes.iter().find(|r| r.id == *recipe_id) else {
                continue;
            };
            if !recipe.can_run_at(facility.facility_type) {
                continue;
            }

            let total_workers: u32 = recipe.workers.values().sum();
            if total_workers == 0 {
                continue;
            }

            let output_value: Price = recipe
                .outputs
                .iter()
                .map(|(good_id, qty)| {
                    let price = settlement.price_ema.get(good_id).copied().unwrap_or(1.0);
                    qty * price
                })
                .sum();

            let per_worker = output_value / total_workers as f64;

            for &skill in recipe.workers.keys() {
                let entry = skill_mvps.entry(skill).or_insert(0.0);
                if per_worker > *entry {
                    *entry = per_worker;
                }
            }
        }

        skill_mvps
    }

    pub(super) fn run_labor_phase_all_settlements(
        &mut self,
        settlement_ids: &[SettlementId],
        recipes: &[Recipe],
        merchants: &mut HashMap<MerchantId, MerchantAgent>,
    ) {
        let mut prepared: HashMap<SettlementId, PreparedLaborSettlement> = HashMap::new();

        for &settlement_id in settlement_ids {
            let Some(prepared_settlement) =
                self.prepare_labor_phase_settlement(settlement_id, recipes, merchants, None)
            else {
                continue;
            };
            prepared.insert(settlement_id, prepared_settlement);
        }

        let initial_owner_budgets: HashMap<MerchantId, f64> = merchants
            .iter()
            .map(|(id, merchant)| (*id, merchant.currency))
            .collect();

        let first_candidates = self.collect_candidate_assignments(&prepared);
        let first_reservation =
            Self::reserve_payable_assignments(first_candidates, &initial_owner_budgets);

        let mut final_prepared = prepared;
        let mut final_payable_by_settlement = first_reservation.payable_by_settlement;

        if !first_reservation.clipped_owners.is_empty() {
            let impacted_settlements: HashSet<SettlementId> = final_prepared
                .iter()
                .filter_map(|(settlement_id, prepared_settlement)| {
                    let affected =
                        prepared_settlement
                            .result
                            .assignments
                            .iter()
                            .any(|assignment| {
                                self.settlements
                                    .get(settlement_id)
                                    .and_then(|s| s.facilities.get(assignment.facility_id))
                                    .map(|f| first_reservation.clipped_owners.contains(&f.owner))
                                    .unwrap_or(false)
                            });
                    if affected { Some(*settlement_id) } else { None }
                })
                .collect();
            let impacted_settlement_ids =
                crate::determinism::sorted_settlement_ids(impacted_settlements.iter().copied());

            let mut reclear_owner_budgets = initial_owner_budgets.clone();
            let payable_settlement_ids = crate::determinism::sorted_settlement_ids(
                final_payable_by_settlement.keys().copied(),
            );
            for settlement_id in payable_settlement_ids {
                let Some(assignments) = final_payable_by_settlement.get(&settlement_id) else {
                    continue;
                };
                if impacted_settlements.contains(&settlement_id) {
                    continue;
                }
                for assignment in assignments {
                    let Some(owner_id) = self
                        .settlements
                        .get(&settlement_id)
                        .and_then(|s| s.facilities.get(assignment.facility_id))
                        .map(|f| f.owner)
                    else {
                        continue;
                    };
                    let entry = reclear_owner_budgets.entry(owner_id).or_insert(0.0);
                    *entry = (*entry - assignment.wage).max(0.0);
                }
            }

            for settlement_id in &impacted_settlement_ids {
                final_prepared.remove(settlement_id);
                final_payable_by_settlement.remove(settlement_id);
            }

            let mut impacted_prepared: HashMap<SettlementId, PreparedLaborSettlement> =
                HashMap::new();
            for settlement_id in impacted_settlement_ids {
                let Some(prepared_settlement) = self.prepare_labor_phase_settlement(
                    settlement_id,
                    recipes,
                    merchants,
                    Some(&reclear_owner_budgets),
                ) else {
                    continue;
                };
                impacted_prepared.insert(settlement_id, prepared_settlement);
            }
            let reclear_candidates = self.collect_candidate_assignments(&impacted_prepared);
            let reclear_reservation =
                Self::reserve_payable_assignments(reclear_candidates, &reclear_owner_budgets);
            final_prepared.extend(impacted_prepared);
            for (settlement_id, assignments) in reclear_reservation.payable_by_settlement {
                final_payable_by_settlement.insert(settlement_id, assignments);
            }
        }

        for &settlement_id in settlement_ids {
            let Some(prepared_settlement) = final_prepared.remove(&settlement_id) else {
                continue;
            };
            let assignments = final_payable_by_settlement
                .remove(&settlement_id)
                .unwrap_or_default();
            self.commit_labor_phase_settlement(
                settlement_id,
                prepared_settlement,
                assignments,
                merchants,
            );
        }
    }

    fn collect_candidate_assignments(
        &self,
        prepared: &HashMap<SettlementId, PreparedLaborSettlement>,
    ) -> Vec<CandidateLaborAssignment> {
        let mut candidates = Vec::new();

        for (settlement_id, prepared_settlement) in prepared {
            if let Some(settlement) = self.settlements.get(settlement_id) {
                for assignment in &prepared_settlement.result.assignments {
                    let Some(owner_id) = settlement
                        .facilities
                        .get(assignment.facility_id)
                        .map(|f| f.owner)
                    else {
                        continue;
                    };

                    candidates.push(CandidateLaborAssignment {
                        owner_id,
                        settlement_name: settlement.info.name.clone(),
                        settlement_id: *settlement_id,
                        assignment: assignment.clone(),
                    });
                }
            }
        }

        candidates.sort_by(|a, b| {
            b.assignment
                .wage
                .partial_cmp(&a.assignment.wage)
                .unwrap()
                .then_with(|| a.owner_id.0.cmp(&b.owner_id.0))
                .then_with(|| a.settlement_name.cmp(&b.settlement_name))
                .then_with(|| a.settlement_id.0.cmp(&b.settlement_id.0))
                .then_with(|| {
                    facility_key_u64(a.assignment.facility_id)
                        .cmp(&facility_key_u64(b.assignment.facility_id))
                })
                .then_with(|| a.assignment.worker_id.cmp(&b.assignment.worker_id))
                .then_with(|| a.assignment.skill.0.cmp(&b.assignment.skill.0))
        });

        candidates
    }

    fn reserve_payable_assignments(
        candidates: Vec<CandidateLaborAssignment>,
        owner_budgets: &HashMap<MerchantId, f64>,
    ) -> LaborReservationResult {
        let mut owner_remaining = owner_budgets.clone();
        let mut payable_by_settlement: HashMap<SettlementId, Vec<Assignment>> = HashMap::new();
        let mut clipped_owners: HashSet<MerchantId> = HashSet::new();

        for candidate in candidates {
            let remaining = owner_remaining.entry(candidate.owner_id).or_insert(0.0);
            if *remaining + 1e-9 < candidate.assignment.wage {
                clipped_owners.insert(candidate.owner_id);
                continue;
            }

            *remaining -= candidate.assignment.wage;
            payable_by_settlement
                .entry(candidate.settlement_id)
                .or_default()
                .push(candidate.assignment);
        }

        LaborReservationResult {
            payable_by_settlement,
            clipped_owners,
        }
    }

    fn prepare_labor_phase_settlement(
        &mut self,
        settlement_id: SettlementId,
        recipes: &[Recipe],
        merchants: &HashMap<MerchantId, MerchantAgent>,
        owner_budget_overrides: Option<&HashMap<MerchantId, f64>>,
    ) -> Option<PreparedLaborSettlement> {
        let settlement = self.settlements.get_mut(&settlement_id)?;

        settlement.update_subsistence_queue();

        let skills = Self::gather_labor_skills(settlement, recipes);

        let wage_seed = if settlement.wage_ema.is_empty() {
            1.0
        } else {
            settlement.wage_ema.values().copied().sum::<f64>() / settlement.wage_ema.len() as f64
        };
        for skill in &skills {
            settlement.wage_ema.entry(skill.id).or_insert(wage_seed);
        }

        let mut facility_skill_bids: HashMap<(FacilityKey, SkillId), (u32, Price)> = HashMap::new();
        let mut bids: Vec<LaborBid> = Vec::new();
        let mut next_bid_id = 0u64;

        let facility_keys = crate::determinism::sorted_facility_keys(settlement.facilities.keys());

        for facility_key in facility_keys {
            let Some(facility) = settlement.facilities.get(facility_key) else {
                continue;
            };

            let merchant_budget = merchants
                .get(&facility.owner)
                .map(|m| m.currency)
                .unwrap_or(0.0);
            let merchant_budget = owner_budget_overrides
                .and_then(|budgets| budgets.get(&facility.owner).copied())
                .unwrap_or(merchant_budget);
            if merchant_budget <= 0.0 {
                continue;
            }

            if settlement.facility_bid_states.get(facility_key).is_none() {
                settlement
                    .facility_bid_states
                    .insert(facility_key, FacilityBidState::default());
            }
            let bid_state = settlement
                .facility_bid_states
                .get(facility_key)
                .cloned()
                .unwrap_or_default();

            let max_workers = facility.capacity.min(50);

            let skill_mvps =
                Self::facility_skill_mvps(settlement, facility, recipes);

            let mut skill_mvp_pairs: Vec<_> = skill_mvps.iter().collect();
            skill_mvp_pairs.sort_by_key(|(s, _)| s.0);

            for (skill_id, mvp) in skill_mvp_pairs {
                let wage_ema = settlement.wage_ema.get(skill_id).copied().unwrap_or(1.0);
                let adaptive_bid = bid_state.get_bid(*skill_id, wage_ema);
                let actual_bid = adaptive_bid.min(*mvp);
                facility_skill_bids.insert((facility_key, *skill_id), (max_workers, *mvp));

                for _ in 0..max_workers {
                    if *mvp > 0.0 {
                        bids.push(LaborBid {
                            id: next_bid_id,
                            facility_id: facility_key,
                            skill: *skill_id,
                            max_wage: actual_bid,
                        });
                        next_bid_id += 1;
                    }
                }
            }
        }

        let subsistence_reservation_by_pop: HashMap<PopKey, Price> =
            if let Some(cfg) = &self.subsistence_reservation {
                let mut employed_ids = Vec::new();
                let mut unemployed_ids = Vec::new();
                for (key, pop) in settlement.pops.iter() {
                    if pop.employed_at.is_some() {
                        employed_ids.push(key);
                    } else {
                        unemployed_ids.push(key);
                    }
                }
                let grain_price_ref = settlement
                    .price_ema
                    .get(&cfg.grain_good)
                    .copied()
                    .unwrap_or(cfg.default_grain_price);
                build_subsistence_reservation_ladder(
                    &employed_ids,
                    &unemployed_ids,
                    grain_price_ref,
                    cfg,
                    &settlement.subsistence_queue,
                )
            } else {
                HashMap::new()
            };

        let mut asks = Vec::new();
        let mut next_ask_id = 0u64;
        let pop_keys = crate::determinism::sorted_pop_keys(settlement.pops.keys());
        for pop_key in &pop_keys {
            let Some(pop) = settlement.pops.get(*pop_key) else {
                continue;
            };
            let reservation = subsistence_reservation_by_pop
                .get(pop_key)
                .copied()
                .map(|r| r.max(pop.min_wage))
                .unwrap_or(pop.min_wage);
            asks.extend(generate_pop_asks_with_min_wage(
                pop,
                pop_key_u64(*pop_key),
                &mut next_ask_id,
                reservation,
            ));
        }

        let facility_budgets: HashMap<FacilityKey, f64> = settlement
            .facilities
            .iter()
            .map(|(k, f)| {
                (
                    k,
                    owner_budget_overrides
                        .and_then(|budgets| budgets.get(&f.owner).copied())
                        .unwrap_or_else(|| {
                            merchants.get(&f.owner).map(|m| m.currency).unwrap_or(0.0)
                        }),
                )
            })
            .collect();

        let result = clear_labor_markets(
            &skills,
            &bids,
            &asks,
            &settlement.wage_ema,
            &facility_budgets,
        );

        let unique_workers = asks.iter().map(|a| a.worker_id).collect::<HashSet<_>>().len() as u32;

        Some(PreparedLaborSettlement {
            skills,
            facility_skill_bids,
            total_workers: unique_workers,
            pop_keys,
            result,
        })
    }

    fn commit_labor_phase_settlement(
        &mut self,
        settlement_id: SettlementId,
        prepared: PreparedLaborSettlement,
        assignments: Vec<Assignment>,
        merchants: &mut HashMap<MerchantId, MerchantAgent>,
    ) {
        let Some(settlement) = self.settlements.get_mut(&settlement_id) else {
            return;
        };

        let assigned_skills: HashSet<SkillId> = assignments.iter().map(|a| a.skill).collect();
        let mut clearing_wages = HashMap::new();
        for skill in assigned_skills {
            if let Some(wage) = prepared.result.clearing_wages.get(&skill).copied() {
                clearing_wages.insert(skill, wage);
            }
        }
        let filtered_result = LaborMarketResult {
            clearing_wages,
            assignments: assignments.clone(),
        };
        debug_assert!(
            assignments.iter().all(|assignment| {
                settlement
                    .facilities
                    .get(assignment.facility_id)
                    .and_then(|facility| merchants.get(&facility.owner))
                    .map(|merchant| merchant.currency + 1e-9 >= assignment.wage)
                    .unwrap_or(false)
            }),
            "labor commit should only receive payable assignments"
        );
        update_wage_emas(&mut settlement.wage_ema, &filtered_result);

        let mut fills: HashMap<(FacilityKey, SkillId), u32> = HashMap::new();
        for assignment in &assignments {
            *fills
                .entry((assignment.facility_id, assignment.skill))
                .or_insert(0) += 1;
        }

        let mut workers_per_merchant: HashMap<MerchantId, u32> = HashMap::new();
        for assignment in &assignments {
            if let Some(facility) = settlement.facilities.get(assignment.facility_id) {
                *workers_per_merchant.entry(facility.owner).or_insert(0) += 1;
            }
        }

        for ((facility_key, skill_id), (wanted, mvp)) in &prepared.facility_skill_bids {
            let filled = fills.get(&(*facility_key, *skill_id)).copied().unwrap_or(0);
            let wage_ema = settlement.wage_ema.get(skill_id).copied().unwrap_or(1.0);
            let adaptive_bid = settlement
                .facility_bid_states
                .get(*facility_key)
                .map(|s| s.get_bid(*skill_id, wage_ema))
                .unwrap_or(wage_ema);
            let unfilled = wanted.saturating_sub(filled);
            let profitable_unfilled = if *mvp > adaptive_bid { unfilled } else { 0 };
            let marginal_profitable_mvp = if profitable_unfilled > 0 {
                Some(*mvp)
            } else {
                None
            };

            if let Some(bid_state) = settlement.facility_bid_states.get_mut(*facility_key) {
                bid_state.record_outcome(
                    *skill_id,
                    filled,
                    profitable_unfilled,
                    marginal_profitable_mvp,
                    *mvp,
                );
            }
        }

        let mut rng = self.rng.clone();
        let bid_state_keys =
            crate::determinism::sorted_facility_keys(settlement.facility_bid_states.keys());
        for facility_key in bid_state_keys {
            let my_merchant = settlement.facilities.get(facility_key).map(|f| f.owner);
            let my_workers = my_merchant
                .and_then(|m| workers_per_merchant.get(&m))
                .copied()
                .unwrap_or(0);
            let can_attract_workers = prepared.total_workers > my_workers;

            for skill in &prepared.skills {
                if !prepared
                    .facility_skill_bids
                    .contains_key(&(facility_key, skill.id))
                {
                    continue;
                }
                let wage_ema = settlement.wage_ema.get(&skill.id).copied().unwrap_or(1.0);
                if let Some(bid_state) = settlement.facility_bid_states.get_mut(facility_key) {
                    bid_state.adjust_bid(&mut rng, skill.id, wage_ema, can_attract_workers);
                }
            }
        }
        self.rng = rng;

        for pop in settlement.pops.values_mut() {
            pop.employed_at = None;
            pop.employed_skill = None;
        }
        for facility in settlement.facilities.values_mut() {
            facility.workers.clear();
        }

        let worker_to_pop: HashMap<u64, PopKey> = prepared
            .pop_keys
            .iter()
            .map(|k| (pop_key_u64(*k), *k))
            .collect();

        for assignment in &assignments {
            let Some(owner_id) = settlement
                .facilities
                .get(assignment.facility_id)
                .map(|f| f.owner)
            else {
                continue;
            };

            let can_pay = merchants
                .get(&owner_id)
                .map(|m| m.currency + 1e-9 >= assignment.wage)
                .unwrap_or(false);
            if !can_pay {
                continue;
            }

            let Some(&pop_key) = worker_to_pop.get(&assignment.worker_id) else {
                continue;
            };

            if let Some(merchant) = merchants.get_mut(&owner_id) {
                merchant.currency -= assignment.wage;
            }

            if let Some(pop) = settlement.pops.get_mut(pop_key) {
                pop.currency += assignment.wage;
                pop.record_income(assignment.wage);
                pop.employed_at = Some(assignment.facility_id);
                pop.employed_skill = Some(assignment.skill);
            }

            if let Some(facility) = settlement.facilities.get_mut(assignment.facility_id) {
                *facility.workers.entry(assignment.skill).or_insert(0) += 1;
            }

            #[cfg(feature = "instrument")]
            tracing::info!(
                target: "assignment",
                tick = self.tick,
                pop_id = assignment.worker_id,
                facility_id = facility_key_u64(assignment.facility_id),
                skill_id = assignment.skill.0,
                wage = assignment.wage,
            );
        }

        for pop in settlement.pops.values_mut() {
            if pop.employed_at.is_none() {
                pop.record_income(0.0);
            }
        }
    }
}
