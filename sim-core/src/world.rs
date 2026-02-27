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

mod labor_phase;
mod market_phase;
mod mortality_phase;
mod production_phase;

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
        let new_unemployed =
            crate::determinism::sorted_pop_keys(self.pops.iter().filter_map(|(k, p)| {
                (p.employed_at.is_none() && !queued.contains(&k)).then_some(k)
            }));
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
        let settlement_ids =
            crate::determinism::sorted_settlement_ids(self.settlements.keys().copied());

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
}
