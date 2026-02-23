use std::collections::{HashMap, HashSet};

use rand::{SeedableRng, rngs::StdRng};
use slotmap::{SecondaryMap, SlotMap};

use crate::accounting::{TickStockFlow, capture_world_flow_snapshot, decompose_tick_flow};
use crate::agents::{MerchantAgent, Pop, Stockpile};
use crate::external::{ExternalMarketConfig, OutsideFlowTotals};
use crate::geography::{Route, Settlement};
use crate::labor::{
    Assignment, FacilityBidState, LaborBid, LaborMarketResult, SkillDef, SkillId,
    SubsistenceReservationConfig, build_subsistence_reservation_ladder, clear_labor_markets,
    generate_pop_asks_with_min_wage, update_wage_emas,
};
use crate::mortality::{MortalityOutcome, check_mortality};
use crate::production::{Facility, FacilityType, Recipe, allocate_recipes, execute_production};
use crate::tick::run_settlement_tick;
use crate::types::{
    FacilityHandle, FacilityKey, GoodId, GoodProfile, MerchantId, PopHandle, PopKey, Price,
    SettlementId, facility_key_u64, pop_key_u64,
};

#[derive(Debug, Clone)]
pub struct SettlementState {
    pub id: SettlementId,
    pub info: Settlement,

    pub pops: SlotMap<PopKey, Pop>,
    pub facilities: SlotMap<FacilityKey, Facility>,

    pub price_ema: HashMap<GoodId, Price>,
    pub wage_ema: HashMap<SkillId, Price>,
    pub facility_bid_states: SecondaryMap<FacilityKey, FacilityBidState>,
    pub subsistence_queue: Vec<PopKey>,
    pub depth_multipliers: HashMap<GoodId, f64>,

    pub owner_facility_counts: HashMap<MerchantId, u32>,
}

impl SettlementState {
    fn new(info: Settlement) -> Self {
        Self {
            id: info.id,
            info,
            pops: SlotMap::with_key(),
            facilities: SlotMap::with_key(),
            price_ema: HashMap::new(),
            wage_ema: HashMap::new(),
            facility_bid_states: SecondaryMap::new(),
            subsistence_queue: Vec::new(),
            depth_multipliers: HashMap::new(),
            owner_facility_counts: HashMap::new(),
        }
    }

    fn update_subsistence_queue(&mut self) {
        let in_pops: HashSet<PopKey> = self
            .pops
            .iter()
            .filter_map(|(k, p)| p.employed_at.is_none().then_some(k))
            .collect();

        self.subsistence_queue.retain(|k| in_pops.contains(k));

        let queued: HashSet<PopKey> = self.subsistence_queue.iter().copied().collect();
        let mut new_unemployed: Vec<PopKey> = self
            .pops
            .iter()
            .filter_map(|(k, p)| (p.employed_at.is_none() && !queued.contains(&k)).then_some(k))
            .collect();
        new_unemployed.sort_by_key(|k| pop_key_u64(*k));
        self.subsistence_queue.extend(new_unemployed);
    }
}

struct PreparedLaborSettlement {
    skills: Vec<SkillDef>,
    facility_skill_bids: HashMap<(FacilityKey, SkillId), (u32, Price)>,
    total_workers: u32,
    pop_keys: Vec<PopKey>,
    result: LaborMarketResult,
}

#[derive(Debug, Clone)]
struct CandidateLaborAssignment {
    owner_id: MerchantId,
    settlement_name: String,
    settlement_id: SettlementId,
    assignment: Assignment,
}

#[derive(Debug)]
struct LaborReservationResult {
    payable_by_settlement: HashMap<SettlementId, Vec<Assignment>>,
    clipped_owners: HashSet<MerchantId>,
}

#[derive(Debug, Clone)]
pub struct World {
    pub tick: u64,
    pub settlements: HashMap<SettlementId, SettlementState>,
    pub routes: Vec<Route>,

    pub merchants: HashMap<MerchantId, MerchantAgent>,

    pub external_market: Option<ExternalMarketConfig>,
    pub subsistence_reservation: Option<SubsistenceReservationConfig>,
    pub mortality_grace_ticks: u64,

    pub outside_flow_totals: OutsideFlowTotals,
    pub stock_flow_history: Vec<TickStockFlow>,

    next_settlement_id: u32,
    next_agent_id: u32,

    rng: StdRng,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        let mut thread_rng = rand::rng();
        Self {
            tick: 0,
            settlements: HashMap::new(),
            routes: Vec::new(),
            merchants: HashMap::new(),
            external_market: None,
            subsistence_reservation: None,
            mortality_grace_ticks: 0,
            outside_flow_totals: OutsideFlowTotals::default(),
            stock_flow_history: Vec::new(),
            next_settlement_id: 0,
            next_agent_id: 0,
            rng: StdRng::from_rng(&mut thread_rng),
        }
    }

    pub fn with_seed(seed: u64) -> Self {
        let mut world = Self::new();
        world.rng = StdRng::seed_from_u64(seed);
        world
    }

    pub fn set_random_seed(&mut self, seed: u64) {
        self.rng = StdRng::seed_from_u64(seed);
    }

    pub fn set_external_market(&mut self, config: ExternalMarketConfig) {
        self.external_market = Some(config);
    }

    pub fn set_subsistence_reservation(&mut self, config: SubsistenceReservationConfig) {
        self.subsistence_reservation = Some(config);
    }

    pub fn add_settlement(
        &mut self,
        name: impl Into<String>,
        position: (f64, f64),
    ) -> SettlementId {
        let id = SettlementId::new(self.next_settlement_id);
        self.next_settlement_id += 1;
        let settlement = Settlement::new(id, name, position);
        self.settlements
            .insert(id, SettlementState::new(settlement));
        id
    }

    pub fn get_settlement(&self, id: SettlementId) -> Option<&Settlement> {
        self.settlements.get(&id).map(|s| &s.info)
    }

    pub fn get_settlement_mut(&mut self, id: SettlementId) -> Option<&mut Settlement> {
        self.settlements.get_mut(&id).map(|s| &mut s.info)
    }

    pub fn add_route(&mut self, from: SettlementId, to: SettlementId, distance: u32) {
        self.routes.push(Route::new(from, to, distance));
    }

    pub fn find_route(&self, from: SettlementId, to: SettlementId) -> Option<&Route> {
        self.routes.iter().find(|r| r.connects(from, to))
    }

    pub fn connected_settlements(&self, settlement_id: SettlementId) -> Vec<SettlementId> {
        self.routes
            .iter()
            .filter_map(|r| {
                if r.from == settlement_id {
                    Some(r.to)
                } else if r.to == settlement_id {
                    Some(r.from)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn add_pop(&mut self, settlement_id: SettlementId) -> Option<PopHandle> {
        let settlement = self.settlements.get_mut(&settlement_id)?;
        let key = settlement.pops.insert(Pop::new());
        Some(PopHandle {
            settlement: settlement_id,
            key,
        })
    }

    pub fn pop(&self, handle: PopHandle) -> Option<&Pop> {
        self.settlements
            .get(&handle.settlement)
            .and_then(|s| s.pops.get(handle.key))
    }

    pub fn pop_mut(&mut self, handle: PopHandle) -> Option<&mut Pop> {
        self.settlements
            .get_mut(&handle.settlement)
            .and_then(|s| s.pops.get_mut(handle.key))
    }

    pub fn add_merchant(&mut self) -> MerchantId {
        let id = MerchantId::new(self.next_agent_id);
        self.next_agent_id += 1;
        self.merchants.insert(id, MerchantAgent::new(id));
        id
    }

    pub fn get_merchant(&self, id: MerchantId) -> Option<&MerchantAgent> {
        self.merchants.get(&id)
    }

    pub fn get_merchant_mut(&mut self, id: MerchantId) -> Option<&mut MerchantAgent> {
        self.merchants.get_mut(&id)
    }

    pub fn add_facility(
        &mut self,
        facility_type: FacilityType,
        settlement_id: SettlementId,
        owner_id: MerchantId,
    ) -> Option<FacilityHandle> {
        if !self.merchants.contains_key(&owner_id) {
            return None;
        }
        let settlement = self.settlements.get_mut(&settlement_id)?;

        let key = settlement
            .facilities
            .insert(Facility::new(facility_type, owner_id));
        settlement
            .facility_bid_states
            .insert(key, FacilityBidState::default());
        *settlement
            .owner_facility_counts
            .entry(owner_id)
            .or_insert(0) += 1;

        let handle = FacilityHandle {
            settlement: settlement_id,
            key,
        };
        self.merchants
            .get_mut(&owner_id)
            .expect("merchant must exist")
            .owned_facilities
            .insert(handle);

        Some(handle)
    }

    pub fn facility(&self, handle: FacilityHandle) -> Option<&Facility> {
        self.settlements
            .get(&handle.settlement)
            .and_then(|s| s.facilities.get(handle.key))
    }

    pub fn facility_mut(&mut self, handle: FacilityHandle) -> Option<&mut Facility> {
        self.settlements
            .get_mut(&handle.settlement)
            .and_then(|s| s.facilities.get_mut(handle.key))
    }

    pub fn pops_at(&self, sid: SettlementId) -> impl Iterator<Item = (PopKey, &Pop)> {
        self.settlements
            .get(&sid)
            .into_iter()
            .flat_map(|s| s.pops.iter())
    }

    pub fn facilities_at(
        &self,
        sid: SettlementId,
    ) -> impl Iterator<Item = (FacilityKey, &Facility)> {
        self.settlements
            .get(&sid)
            .into_iter()
            .flat_map(|s| s.facilities.iter())
    }

    pub fn merchants_at(&self, sid: SettlementId) -> impl Iterator<Item = MerchantId> + '_ {
        self.settlements
            .get(&sid)
            .into_iter()
            .flat_map(|s| s.owner_facility_counts.keys().copied())
    }

    pub fn get_price(&self, settlement_id: SettlementId, good: GoodId) -> Price {
        self.settlements
            .get(&settlement_id)
            .and_then(|s| s.price_ema.get(&good).copied())
            .unwrap_or(1.0)
    }

    pub fn update_price(&mut self, settlement_id: SettlementId, good: GoodId, price: Price) {
        if let Some(settlement) = self.settlements.get_mut(&settlement_id) {
            let ema = settlement.price_ema.entry(good).or_insert(price);
            *ema = 0.7 * *ema + 0.3 * price;
        }
    }

    pub fn run_tick(
        &mut self,
        good_profiles: &[GoodProfile],
        needs: &HashMap<String, crate::needs::Need>,
        recipes: &[Recipe],
    ) {
        self.tick += 1;
        let pre_tick_snapshot = capture_world_flow_snapshot(self);

        let mut merchants = std::mem::take(&mut self.merchants);
        let mut settlement_ids: Vec<SettlementId> = self.settlements.keys().copied().collect();
        settlement_ids.sort_by_key(|id| id.0);

        self.run_labor_phase_all_settlements(&settlement_ids, recipes, &mut merchants);

        for &settlement_id in &settlement_ids {
            self.run_production_phase_settlement(settlement_id, recipes, &mut merchants);
        }

        for &settlement_id in &settlement_ids {
            self.run_market_phase_settlement(settlement_id, good_profiles, needs, &mut merchants);
        }

        for &settlement_id in &settlement_ids {
            self.run_mortality_phase_settlement(settlement_id);
        }

        self.merchants = merchants;

        let post_tick_snapshot = capture_world_flow_snapshot(self);
        let tick_flow = decompose_tick_flow(self.tick, &pre_tick_snapshot, &post_tick_snapshot);
        self.stock_flow_history.push(tick_flow);
    }

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

    fn facility_recipe_skills_and_mvp(
        settlement: &SettlementState,
        facility: &Facility,
        recipes: &[Recipe],
    ) -> (Vec<SkillId>, Price) {
        let Some(recipe) = facility
            .recipe_priorities
            .iter()
            .find_map(|rid| recipes.iter().find(|r| r.id == *rid))
        else {
            return (Vec::new(), 1.0);
        };

        let mut relevant_skills: Vec<SkillId> = recipe.workers.keys().copied().collect();
        relevant_skills.sort_by_key(|s| s.0);

        let total_workers: u32 = recipe.workers.values().sum();
        if total_workers == 0 {
            return (relevant_skills, 1.0);
        }

        let output_value: Price = recipe
            .outputs
            .iter()
            .map(|(good_id, qty)| {
                let price = settlement.price_ema.get(good_id).copied().unwrap_or(1.0);
                qty * price
            })
            .sum();

        (relevant_skills, output_value / total_workers as f64)
    }

    fn run_labor_phase_all_settlements(
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

            let mut reclear_owner_budgets = initial_owner_budgets.clone();
            for (settlement_id, assignments) in &final_payable_by_settlement {
                if impacted_settlements.contains(settlement_id) {
                    continue;
                }
                for assignment in assignments {
                    let Some(owner_id) = self
                        .settlements
                        .get(settlement_id)
                        .and_then(|s| s.facilities.get(assignment.facility_id))
                        .map(|f| f.owner)
                    else {
                        continue;
                    };
                    let entry = reclear_owner_budgets.entry(owner_id).or_insert(0.0);
                    *entry = (*entry - assignment.wage).max(0.0);
                }
            }

            for settlement_id in &impacted_settlements {
                final_prepared.remove(settlement_id);
                final_payable_by_settlement.remove(settlement_id);
            }

            let mut impacted_prepared: HashMap<SettlementId, PreparedLaborSettlement> =
                HashMap::new();
            for settlement_id in impacted_settlements {
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

        let mut facility_keys: Vec<FacilityKey> = settlement.facilities.keys().collect();
        facility_keys.sort_by_key(|k| facility_key_u64(*k));

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

            let (facility_skills, mvp) =
                Self::facility_recipe_skills_and_mvp(settlement, facility, recipes);

            for skill_id in facility_skills {
                let wage_ema = settlement.wage_ema.get(&skill_id).copied().unwrap_or(1.0);
                let adaptive_bid = bid_state.get_bid(skill_id, wage_ema);
                let actual_bid = adaptive_bid.min(mvp);
                facility_skill_bids.insert((facility_key, skill_id), (max_workers, mvp));

                for _ in 0..max_workers {
                    if mvp > 0.0 {
                        bids.push(LaborBid {
                            id: next_bid_id,
                            facility_id: facility_key,
                            skill: skill_id,
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
        let mut pop_keys: Vec<PopKey> = settlement.pops.keys().collect();
        pop_keys.sort_by_key(|k| pop_key_u64(*k));
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

        Some(PreparedLaborSettlement {
            skills,
            facility_skill_bids,
            total_workers: asks.len() as u32,
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
        let mut bid_state_keys: Vec<FacilityKey> = settlement.facility_bid_states.keys().collect();
        bid_state_keys.sort_by_key(|k| facility_key_u64(*k));
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

    fn run_production_phase_settlement(
        &mut self,
        settlement_id: SettlementId,
        recipes: &[Recipe],
        merchants: &mut HashMap<MerchantId, MerchantAgent>,
    ) {
        let Some(settlement) = self.settlements.get_mut(&settlement_id) else {
            return;
        };

        let mut production_totals: HashMap<(MerchantId, GoodId), f64> = HashMap::new();
        let mut facility_keys: Vec<FacilityKey> = settlement.facilities.keys().collect();
        facility_keys.sort_by_key(|k| facility_key_u64(*k));

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

        for ((merchant_id, good_id), total_qty) in production_totals {
            if let Some(merchant) = merchants.get_mut(&merchant_id) {
                merchant.record_production(settlement_id, good_id, total_qty);
            }
        }
    }

    fn run_market_phase_settlement(
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

        let mut merchant_ids: Vec<MerchantId> =
            settlement.owner_facility_counts.keys().copied().collect();
        merchant_ids.sort_by_key(|id| id.0);
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

    fn run_mortality_phase_settlement(&mut self, settlement_id: SettlementId) {
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

        let mut pop_keys: Vec<PopKey> = settlement.pops.keys().collect();
        pop_keys.sort_by_key(|k| pop_key_u64(*k));

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
            let mut heirs: Vec<PopKey> = settlement.pops.keys().collect();
            heirs.sort_by_key(|k| pop_key_u64(*k));
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
