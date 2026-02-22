use std::collections::{HashMap, HashSet};

use rand::{SeedableRng, rngs::StdRng};
use slotmap::{SecondaryMap, SlotMap};

use crate::accounting::{TickStockFlow, capture_world_flow_snapshot, decompose_tick_flow};
use crate::agents::{MerchantAgent, Pop, Stockpile};
use crate::external::{ExternalMarketConfig, OutsideFlowTotals};
use crate::geography::{Route, Settlement};
use crate::labor::{
    FacilityBidState, LaborBid, SkillDef, SkillId, SubsistenceReservationConfig,
    build_subsistence_reservation_ladder, clear_labor_markets, generate_pop_asks_with_min_wage,
    update_wage_emas,
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

        for settlement_id in settlement_ids {
            self.run_labor_phase_settlement(settlement_id, recipes, &mut merchants);
            self.run_production_phase_settlement(settlement_id, recipes, &mut merchants);
            self.run_market_phase_settlement(settlement_id, good_profiles, needs, &mut merchants);
            self.run_mortality_phase_settlement(settlement_id);
        }

        self.merchants = merchants;

        let post_tick_snapshot = capture_world_flow_snapshot(self);
        let tick_flow = decompose_tick_flow(self.tick, &pre_tick_snapshot, &post_tick_snapshot);
        self.stock_flow_history.push(tick_flow);
    }

    fn run_labor_phase_settlement(
        &mut self,
        settlement_id: SettlementId,
        recipes: &[Recipe],
        merchants: &mut HashMap<MerchantId, MerchantAgent>,
    ) {
        let Some(settlement) = self.settlements.get_mut(&settlement_id) else {
            return;
        };

        settlement.update_subsistence_queue();

        let mut skills: Vec<SkillDef> = settlement
            .wage_ema
            .keys()
            .map(|&id| SkillDef {
                id,
                name: String::new(),
                parent: None,
            })
            .collect();
        skills.sort_by_key(|s| s.id.0);
        if skills.is_empty() {
            return;
        }

        let output_price = settlement.price_ema.values().copied().next().unwrap_or(1.0);

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

            let output_per_worker = facility
                .recipe_priorities
                .first()
                .and_then(|rid| recipes.iter().find(|r| r.id == *rid))
                .map(|recipe| {
                    let total_output: f64 = recipe.outputs.iter().map(|(_, qty)| qty).sum();
                    let total_workers: u32 = recipe.workers.values().sum();
                    if total_workers > 0 {
                        total_output / total_workers as f64
                    } else {
                        1.0
                    }
                })
                .unwrap_or(1.0);

            for skill in &skills {
                let mvp = output_per_worker * output_price;
                let wage_ema = settlement.wage_ema.get(&skill.id).copied().unwrap_or(1.0);
                let adaptive_bid = bid_state.get_bid(skill.id, wage_ema);
                let actual_bid = adaptive_bid.min(mvp);
                facility_skill_bids.insert((facility_key, skill.id), (max_workers, mvp));

                for _ in 0..max_workers {
                    if mvp > 0.0 {
                        bids.push(LaborBid {
                            id: next_bid_id,
                            facility_id: facility_key,
                            skill: skill.id,
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
                    merchants.get(&f.owner).map(|m| m.currency).unwrap_or(0.0),
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
        update_wage_emas(&mut settlement.wage_ema, &result);

        for pop in settlement.pops.values_mut() {
            pop.employed_at = None;
        }
        for facility in settlement.facilities.values_mut() {
            facility.workers.clear();
        }

        let worker_to_pop: HashMap<u64, PopKey> =
            pop_keys.iter().map(|k| (pop_key_u64(*k), *k)).collect();

        for assignment in &result.assignments {
            let Some(&pop_key) = worker_to_pop.get(&assignment.worker_id) else {
                continue;
            };
            let Some(pop) = settlement.pops.get_mut(pop_key) else {
                continue;
            };
            pop.employed_at = Some(assignment.facility_id);

            let owner = settlement
                .facilities
                .get(assignment.facility_id)
                .map(|f| f.owner);
            if let Some(facility) = settlement.facilities.get_mut(assignment.facility_id) {
                *facility.workers.entry(assignment.skill).or_insert(0) += 1;
            }

            if let Some(owner_id) = owner
                && let Some(merchant) = merchants.get_mut(&owner_id)
            {
                if merchant.currency >= assignment.wage {
                    merchant.currency -= assignment.wage;
                    pop.currency += assignment.wage;
                    pop.record_income(assignment.wage);
                } else {
                    pop.record_income(0.0);
                }
            }
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

        let merchant_ids: Vec<MerchantId> =
            settlement.owner_facility_counts.keys().copied().collect();
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

        let mut outcomes: Vec<(PopKey, MortalityOutcome)> = Vec::with_capacity(pop_keys.len());
        for pop_key in &pop_keys {
            let Some(pop) = settlement.pops.get(*pop_key) else {
                continue;
            };
            let food_satisfaction = pop.need_satisfaction.get("food").copied().unwrap_or(0.0);
            let outcome = check_mortality(&mut rng, food_satisfaction);
            outcomes.push((*pop_key, outcome));
        }

        let mut dead_pops: Vec<PopKey> = Vec::new();
        let mut children: Vec<Pop> = Vec::new();

        for (pop_key, outcome) in outcomes {
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
