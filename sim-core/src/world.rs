// World state for the pops economic simulation

use std::collections::HashMap;

use crate::agents::{MerchantAgent, Pop, Stockpile};
use crate::geography::{Route, Settlement};
use crate::labor::{FacilityBidState, SkillId};
use crate::production::{Facility, FacilityType, Recipe, allocate_recipes, execute_production};
use crate::types::{FacilityId, GoodId, MerchantId, PopId, Price, SettlementId};

/// Complete state of the economic simulation
#[derive(Debug, Clone)]
pub struct World {
    pub tick: u64,

    // Geography
    pub settlements: HashMap<SettlementId, Settlement>,
    pub routes: Vec<Route>,

    // Agents
    pub pops: HashMap<PopId, Pop>,
    pub merchants: HashMap<MerchantId, MerchantAgent>,

    // Production
    pub facilities: HashMap<FacilityId, Facility>,

    // Market state per settlement
    pub price_ema: HashMap<(SettlementId, GoodId), Price>,

    // Labor market state
    pub wage_ema: HashMap<SkillId, Price>,
    /// Facility bid states for adaptive wage bidding
    pub facility_bid_states: HashMap<FacilityId, FacilityBidState>,

    // ID counters
    next_settlement_id: u32,
    next_agent_id: u32, // shared counter for PopId and MerchantId to avoid collisions
    next_facility_id: u32,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            tick: 0,
            settlements: HashMap::new(),
            routes: Vec::new(),
            pops: HashMap::new(),
            merchants: HashMap::new(),
            facilities: HashMap::new(),
            price_ema: HashMap::new(),
            wage_ema: HashMap::new(),
            facility_bid_states: HashMap::new(),
            next_settlement_id: 0,
            next_agent_id: 0, // shared counter for PopId and MerchantId
            next_facility_id: 0,
        }
    }

    // === Simulation Tick ===

    /// Run one simulation tick across all settlements.
    ///
    /// Tick phases:
    /// 0. Labor - pay wages to employed pops
    /// 1. Production - facilities produce goods using workers and inputs
    /// 2. Consumption - pops consume goods to satisfy needs
    /// 3. Market clearing - call auction for each settlement
    /// 4. Price EMA update
    pub fn run_tick(
        &mut self,
        good_profiles: &[crate::types::GoodProfile],
        needs: &std::collections::HashMap<String, crate::needs::Need>,
        recipes: &[Recipe],
    ) {
        use crate::tick::run_settlement_tick;

        self.tick += 1;

        // === 0. LABOR PHASE ===
        self.run_labor_phase();

        // === 1. PRODUCTION PHASE ===
        self.run_production_phase(recipes);

        // Process each settlement
        let settlement_ids: Vec<SettlementId> = self.settlements.keys().copied().collect();

        for settlement_id in settlement_ids {
            // Extract price EMA for this settlement
            let mut settlement_prices: HashMap<GoodId, Price> = good_profiles
                .iter()
                .map(|gp| {
                    let price = self
                        .price_ema
                        .get(&(settlement_id, gp.good))
                        .copied()
                        .unwrap_or(1.0);
                    (gp.good, price)
                })
                .collect();

            // Get pop IDs at this settlement
            let pop_ids: Vec<PopId> = self
                .settlements
                .get(&settlement_id)
                .map(|s| s.pop_ids.clone())
                .unwrap_or_default();

            // Get merchant IDs with presence at this settlement
            let merchant_ids: Vec<MerchantId> = self.merchants_at_settlement(settlement_id);

            // Temporarily extract pops and merchants to get mutable refs
            let mut extracted_pops: Vec<(PopId, Pop)> = pop_ids
                .iter()
                .filter_map(|id| self.pops.remove(id).map(|p| (*id, p)))
                .collect();

            let mut extracted_merchants: Vec<(MerchantId, MerchantAgent)> = merchant_ids
                .iter()
                .filter_map(|id| self.merchants.remove(id).map(|m| (*id, m)))
                .collect();

            // Create mutable reference slices
            let mut pop_refs: Vec<&mut Pop> = extracted_pops.iter_mut().map(|(_, p)| p).collect();
            let mut merchant_refs: Vec<&mut MerchantAgent> =
                extracted_merchants.iter_mut().map(|(_, m)| m).collect();

            // Run the settlement tick
            let _result = run_settlement_tick(
                self.tick,
                settlement_id,
                &mut pop_refs,
                &mut merchant_refs,
                good_profiles,
                needs,
                &mut settlement_prices,
            );

            // Put pops and merchants back
            for (id, pop) in extracted_pops {
                self.pops.insert(id, pop);
            }
            for (id, merchant) in extracted_merchants {
                self.merchants.insert(id, merchant);
            }

            // Merge settlement prices back into world price_ema
            for (good, price) in settlement_prices {
                self.price_ema.insert((settlement_id, good), price);
            }
        }

        // === 5. MORTALITY PHASE ===
        self.run_mortality_phase();
    }

    // === Settlement Management ===

    /// Add a settlement to the world, returns its ID
    pub fn add_settlement(
        &mut self,
        name: impl Into<String>,
        position: (f64, f64),
    ) -> SettlementId {
        let id = SettlementId::new(self.next_settlement_id);
        self.next_settlement_id += 1;

        let settlement = Settlement::new(id, name, position);
        self.settlements.insert(id, settlement);
        id
    }

    /// Get a settlement by ID
    pub fn get_settlement(&self, id: SettlementId) -> Option<&Settlement> {
        self.settlements.get(&id)
    }

    /// Get a mutable reference to a settlement
    pub fn get_settlement_mut(&mut self, id: SettlementId) -> Option<&mut Settlement> {
        self.settlements.get_mut(&id)
    }

    // === Route Management ===

    /// Add a route between two settlements
    pub fn add_route(&mut self, from: SettlementId, to: SettlementId, distance: u32) {
        self.routes.push(Route::new(from, to, distance));
    }

    /// Find a route between two settlements
    pub fn find_route(&self, from: SettlementId, to: SettlementId) -> Option<&Route> {
        self.routes.iter().find(|r| r.connects(from, to))
    }

    /// Get all settlements connected to a given settlement
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

    // === Pop Management ===

    /// Add a pop to a settlement, returns its ID
    pub fn add_pop(&mut self, settlement_id: SettlementId) -> Option<PopId> {
        let settlement = self.settlements.get_mut(&settlement_id)?;

        let id = PopId::new(self.next_agent_id);
        self.next_agent_id += 1;

        let pop = Pop::new(id, settlement_id);
        self.pops.insert(id, pop);
        settlement.pop_ids.push(id);

        Some(id)
    }

    /// Get a pop by ID
    pub fn get_pop(&self, id: PopId) -> Option<&Pop> {
        self.pops.get(&id)
    }

    /// Get a mutable reference to a pop
    pub fn get_pop_mut(&mut self, id: PopId) -> Option<&mut Pop> {
        self.pops.get_mut(&id)
    }

    /// Get all pops at a settlement
    pub fn pops_at_settlement(&self, settlement_id: SettlementId) -> Vec<&Pop> {
        self.settlements
            .get(&settlement_id)
            .map(|s| {
                s.pop_ids
                    .iter()
                    .filter_map(|id| self.pops.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    // === Merchant Management ===

    /// Add a merchant to the world, returns its ID
    pub fn add_merchant(&mut self) -> MerchantId {
        let id = MerchantId::new(self.next_agent_id);
        self.next_agent_id += 1;

        let merchant = MerchantAgent::new(id);
        self.merchants.insert(id, merchant);
        id
    }

    /// Get a merchant by ID
    pub fn get_merchant(&self, id: MerchantId) -> Option<&MerchantAgent> {
        self.merchants.get(&id)
    }

    /// Get a mutable reference to a merchant
    pub fn get_merchant_mut(&mut self, id: MerchantId) -> Option<&mut MerchantAgent> {
        self.merchants.get_mut(&id)
    }

    // === Facility Management ===

    /// Add a facility at a settlement owned by a merchant, returns its ID
    pub fn add_facility(
        &mut self,
        facility_type: FacilityType,
        settlement_id: SettlementId,
        owner_id: MerchantId,
    ) -> Option<FacilityId> {
        // Verify settlement and merchant exist
        if !self.settlements.contains_key(&settlement_id) {
            return None;
        }
        let merchant = self.merchants.get_mut(&owner_id)?;

        let id = FacilityId::new(self.next_facility_id);
        self.next_facility_id += 1;

        let facility = Facility::new(id, facility_type, settlement_id, owner_id);
        self.facilities.insert(id, facility);

        // Track ownership on the merchant
        merchant.facility_ids.insert(id);

        Some(id)
    }

    /// Get a facility by ID
    pub fn get_facility(&self, id: FacilityId) -> Option<&Facility> {
        self.facilities.get(&id)
    }

    /// Get a mutable reference to a facility
    pub fn get_facility_mut(&mut self, id: FacilityId) -> Option<&mut Facility> {
        self.facilities.get_mut(&id)
    }

    /// Get all facilities at a settlement
    pub fn facilities_at_settlement(&self, settlement_id: SettlementId) -> Vec<&Facility> {
        self.facilities
            .values()
            .filter(|f| f.settlement == settlement_id)
            .collect()
    }

    /// Get all facilities owned by a merchant
    pub fn facilities_owned_by(&self, owner_id: MerchantId) -> Vec<&Facility> {
        self.facilities
            .values()
            .filter(|f| f.owner == owner_id)
            .collect()
    }

    /// Check if a merchant has presence at a settlement (owns a facility there)
    pub fn merchant_has_facility_at(
        &self,
        merchant_id: MerchantId,
        settlement_id: SettlementId,
    ) -> bool {
        self.facilities
            .values()
            .any(|f| f.owner == merchant_id && f.settlement == settlement_id)
    }

    /// Get all merchants with presence at a settlement (via facility ownership)
    pub fn merchants_at_settlement(&self, settlement_id: SettlementId) -> Vec<MerchantId> {
        self.facilities
            .values()
            .filter(|f| f.settlement == settlement_id)
            .map(|f| f.owner)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    }

    // === Price Management ===

    /// Get market price for a good at a settlement, or default
    pub fn get_price(&self, settlement_id: SettlementId, good: GoodId) -> Price {
        self.price_ema
            .get(&(settlement_id, good))
            .copied()
            .unwrap_or(1.0) // Default price
    }

    /// Update price EMA after trading
    pub fn update_price(&mut self, settlement_id: SettlementId, good: GoodId, price: Price) {
        let ema = self.price_ema.entry((settlement_id, good)).or_insert(price);
        *ema = 0.7 * *ema + 0.3 * price;
    }

    // === Labor ===

    /// Pay wages to employed pops.
    ///
    /// # Current Implementation (Simplified)
    ///
    /// For v1, we use a simplified wage payment model:
    /// - Each employed pop receives wages directly from the merchant who owns their facility
    /// - Wage amount is based on the wage_ema for the pop's primary skill
    /// - No labor market clearing - assignments are static
    ///
    /// # Full Facility Treasury Design (Future)
    ///
    /// The complete design separates facility and merchant finances:
    ///
    /// ```text
    /// Merchant funds facility treasury
    ///          ↓
    /// Facility.currency (treasury)
    ///          ↓
    /// Facility pays wages → Pop.currency
    ///          ↓
    /// Production outputs → Merchant stockpile
    ///          ↓
    /// Merchant sells goods → Merchant.currency
    ///          ↓
    /// (cycle repeats)
    /// ```
    ///
    /// Treasury rules:
    /// - Facility maintains 1-2 ticks of wages as buffer
    /// - Revenue from production goes to facility treasury
    /// - Excess above threshold flows to merchant
    /// - If treasury insufficient, merchant must inject funds or facility pauses
    ///
    /// This creates interesting cash flow management decisions:
    /// - Merchant must allocate capital across facilities
    /// - Facilities can fail due to liquidity crises
    /// - Player/AI decides when to fund vs abandon struggling facilities
    fn run_labor_phase(&mut self) {
        use crate::labor::{
            LaborBid, SkillDef, clear_labor_markets, generate_pop_asks, update_wage_emas,
        };

        // Collect all skills in use
        let skills: Vec<SkillDef> = self
            .wage_ema
            .keys()
            .map(|&id| SkillDef {
                id,
                name: String::new(),
                parent: None,
            })
            .collect();

        if skills.is_empty() {
            return;
        }

        // === PHASE 1: Generate bids with adaptive pricing ===
        // Track (facility_id, skill) -> (bids_generated, mvp) for outcome computation
        let mut facility_skill_bids: HashMap<(FacilityId, SkillId), (u32, Price)> = HashMap::new();
        let mut bids: Vec<LaborBid> = Vec::new();
        let mut next_bid_id = 0u64;

        for facility in self.facilities.values() {
            // Get output price for this facility's settlement (simplified MVP)
            let output_price = self
                .price_ema
                .iter()
                .find(|((sid, _), _)| *sid == facility.settlement)
                .map(|(_, &p)| p)
                .unwrap_or(1.0);

            // Get merchant's budget
            let merchant_budget = self
                .merchants
                .get(&facility.owner)
                .map(|m| m.currency)
                .unwrap_or(0.0);

            if merchant_budget <= 0.0 {
                continue;
            }

            // Get or create bid state for this facility
            let bid_state = self.facility_bid_states.entry(facility.id).or_default();

            let max_workers = facility.capacity.min(50);

            for skill in &skills {
                // MVP = output_price for all slots (simplified, no diminishing returns)
                let mvp = output_price;

                // Get adaptive bid from state
                let wage_ema = self.wage_ema.get(&skill.id).copied().unwrap_or(1.0);
                let adaptive_bid = bid_state.get_bid(skill.id, wage_ema);

                // Actual bid = min(adaptive_bid, mvp)
                let actual_bid = adaptive_bid.min(mvp);

                // Track how many bids we're generating
                facility_skill_bids.insert((facility.id, skill.id), (max_workers, mvp));

                for _ in 0..max_workers {
                    if mvp > 0.0 {
                        bids.push(LaborBid {
                            id: next_bid_id,
                            facility_id: facility.id,
                            skill: skill.id,
                            max_wage: actual_bid,
                        });

                        #[cfg(feature = "instrument")]
                        tracing::info!(
                            target: "labor_bid",
                            tick = self.tick,
                            bid_id = next_bid_id,
                            facility_id = facility.id.0,
                            skill_id = skill.id.0,
                            max_wage = actual_bid,
                            mvp = mvp,
                            adaptive_bid = adaptive_bid,
                        );

                        next_bid_id += 1;
                    }
                }
            }
        }

        // === PHASE 2: Generate pop asks ===
        let mut asks = Vec::new();
        let mut next_ask_id = 0u64;
        for pop in self.pops.values() {
            let mut pop_asks = generate_pop_asks(pop, &mut next_ask_id);

            #[cfg(feature = "instrument")]
            for ask in &pop_asks {
                tracing::info!(
                    target: "labor_ask",
                    tick = self.tick,
                    ask_id = ask.id,
                    pop_id = ask.worker_id,
                    skill_id = ask.skill.0,
                    min_wage = ask.min_wage,
                );
            }

            asks.append(&mut pop_asks);
        }

        // === PHASE 3: Clear labor markets ===
        let facility_budgets: HashMap<FacilityId, f64> = self
            .facilities
            .iter()
            .map(|(fid, f)| {
                let budget = self
                    .merchants
                    .get(&f.owner)
                    .map(|m| m.currency)
                    .unwrap_or(0.0);
                (*fid, budget)
            })
            .collect();

        let result = clear_labor_markets(&skills, &bids, &asks, &self.wage_ema, &facility_budgets);

        // Debug: print labor market clearing results
        #[cfg(debug_assertions)]
        if !result.assignments.is_empty() || !bids.is_empty() || !asks.is_empty() {
            // eprintln!(
            //     "[Labor] bids={}, asks={}, assignments={}",
            //     bids.len(),
            //     asks.len(),
            //     result.assignments.len()
            // );
            // for a in &result.assignments {
            //     eprintln!(
            //         "[Labor]   Assignment: worker={} -> facility={:?} skill={:?} wage={}",
            //         a.worker_id, a.facility_id, a.skill, a.wage
            //     );
            // }
        }

        // Update wage EMAs
        update_wage_emas(&mut self.wage_ema, &result);

        // === PHASE 4: Count fills per (facility, skill) ===
        let mut fills: HashMap<(FacilityId, SkillId), u32> = HashMap::new();
        for assignment in &result.assignments {
            *fills
                .entry((assignment.facility_id, assignment.skill))
                .or_insert(0) += 1;
        }

        // === PHASE 5: Record outcomes and adjust bids ===
        // Compute global excess workers
        let total_workers: u32 = asks.len() as u32;
        let total_jobs: u32 = bids.len() as u32;
        let global_excess_workers = total_workers > total_jobs;

        for ((facility_id, skill_id), (wanted, mvp)) in &facility_skill_bids {
            let filled = fills.get(&(*facility_id, *skill_id)).copied().unwrap_or(0);

            // Get adaptive bid for this skill
            let bid_state = self.facility_bid_states.get(facility_id);
            let wage_ema = self.wage_ema.get(skill_id).copied().unwrap_or(1.0);
            let adaptive_bid = bid_state
                .map(|s| s.get_bid(*skill_id, wage_ema))
                .unwrap_or(wage_ema);

            // Compute profitable_unfilled: unfilled slots where MVP > adaptive_bid
            let unfilled = wanted.saturating_sub(filled);
            let profitable_unfilled = if *mvp > adaptive_bid { unfilled } else { 0 };

            // Marginal profitable MVP (since all slots have same MVP, it's just mvp if profitable)
            let marginal_profitable_mvp = if profitable_unfilled > 0 {
                Some(*mvp)
            } else {
                None
            };

            // Record outcome
            if let Some(bid_state) = self.facility_bid_states.get_mut(facility_id) {
                bid_state.record_outcome(
                    *skill_id,
                    filled,
                    profitable_unfilled,
                    marginal_profitable_mvp,
                );

                #[cfg(feature = "instrument")]
                tracing::info!(
                    target: "skill_outcome",
                    tick = self.tick,
                    facility_id = facility_id.0,
                    skill_id = skill_id.0,
                    wanted = *wanted,
                    filled = filled,
                    profitable_unfilled = profitable_unfilled,
                    marginal_mvp = marginal_profitable_mvp.unwrap_or(0.0),
                );
            }
        }

        // Adjust bids for next tick
        for (facility_id, bid_state) in self.facility_bid_states.iter_mut() {
            for skill in &skills {
                let wage_ema = self.wage_ema.get(&skill.id).copied().unwrap_or(1.0);
                // Only adjust if this facility had bids for this skill
                if facility_skill_bids.contains_key(&(*facility_id, skill.id)) {
                    bid_state.adjust_bid(skill.id, wage_ema, global_excess_workers);
                }
            }
        }

        // === PHASE 6: Apply assignments ===
        // Clear existing employment
        for pop in self.pops.values_mut() {
            pop.employed_at = None;
        }
        for facility in self.facilities.values_mut() {
            facility.workers.clear();
        }

        // Apply new assignments and pay wages
        for assignment in &result.assignments {
            let pop_id = PopId::new(assignment.worker_id);

            #[cfg(feature = "instrument")]
            tracing::info!(
                target: "assignment",
                tick = self.tick,
                pop_id = assignment.worker_id,
                facility_id = assignment.facility_id.0,
                skill_id = assignment.skill.0,
                wage = assignment.wage,
            );

            if let Some(pop) = self.pops.get_mut(&pop_id) {
                pop.employed_at = Some(assignment.facility_id);

                if let Some(facility) = self.facilities.get_mut(&assignment.facility_id) {
                    *facility.workers.entry(assignment.skill).or_insert(0) += 1;

                    if let Some(merchant) = self.merchants.get_mut(&facility.owner) {
                        if merchant.currency >= assignment.wage {
                            merchant.currency -= assignment.wage;
                            pop.currency += assignment.wage;
                            pop.record_income(assignment.wage);
                        } else {
                            pop.record_income(0.0);
                        }
                    }
                }
            }
        }

        // Record zero income for unemployed pops
        for pop in self.pops.values_mut() {
            if pop.employed_at.is_none() {
                pop.record_income(0.0);
            }
        }
    }

    // === Production ===

    /// Run production for all facilities.
    ///
    /// For each facility:
    /// 1. Get the merchant's stockpile at the facility's settlement
    /// 2. Allocate recipes based on priority, workers, inputs, capacity
    /// 3. Execute production (consume inputs, produce outputs)
    /// 4. Apply quality multiplier from resource slot
    fn run_production_phase(&mut self, recipes: &[Recipe]) {
        let facility_ids: Vec<FacilityId> = self.facilities.keys().copied().collect();

        // Accumulate production per (merchant, settlement, good) to update EMA once per tick
        let mut production_totals: HashMap<(MerchantId, SettlementId, GoodId), f64> = HashMap::new();

        for facility_id in facility_ids {
            // Get facility info (immutable borrow)
            let (settlement_id, merchant_id, quality_multiplier) = {
                let facility = match self.facilities.get(&facility_id) {
                    Some(f) => f,
                    None => continue,
                };

                // Get quality multiplier from resource slot
                let quality = self
                    .settlements
                    .get(&facility.settlement)
                    .and_then(|s| s.get_facility_slot(facility_id))
                    .map(|slot| slot.quality.multiplier())
                    .unwrap_or(1.0);

                (facility.settlement, facility.owner, quality)
            };

            // Get or create stockpile for this merchant at this settlement
            let merchant = match self.merchants.get_mut(&merchant_id) {
                Some(m) => m,
                None => continue,
            };

            // Ensure merchant has a stockpile at this settlement
            let stockpile = merchant
                .stockpiles
                .entry(settlement_id)
                .or_insert_with(Stockpile::new);

            // Allocate recipes (need immutable facility ref)
            let facility = self.facilities.get(&facility_id).unwrap();
            let allocation = allocate_recipes(facility, recipes, stockpile);

            // Execute production (mutates stockpile)
            let stockpile = self
                .merchants
                .get_mut(&merchant_id)
                .unwrap()
                .stockpiles
                .get_mut(&settlement_id)
                .unwrap();

            let result = execute_production(&allocation, recipes, stockpile, quality_multiplier);

            #[cfg(feature = "instrument")]
            {
                // Log each recipe run
                for (recipe_id, runs) in &allocation.runs {
                    if *runs > 0 {
                        tracing::info!(
                            target: "production",
                            tick = self.tick,
                            facility_id = facility_id.0,
                            settlement_id = settlement_id.0,
                            recipe_id = recipe_id.0,
                            runs = *runs,
                            quality_multiplier = quality_multiplier,
                        );
                    }
                }

                // Log inputs consumed
                for (good_id, qty) in &result.inputs_consumed {
                    if *qty > 0.0 {
                        tracing::info!(
                            target: "production_io",
                            tick = self.tick,
                            facility_id = facility_id.0,
                            good_id = *good_id,
                            direction = "input",
                            quantity = *qty,
                        );
                    }
                }

                // Log outputs produced
                for (good_id, qty) in &result.outputs_produced {
                    if *qty > 0.0 {
                        tracing::info!(
                            target: "production_io",
                            tick = self.tick,
                            facility_id = facility_id.0,
                            good_id = *good_id,
                            direction = "output",
                            quantity = *qty,
                        );
                    }
                }
            }

            // Accumulate production for this merchant/settlement/good
            for (&good_id, &qty) in &result.outputs_produced {
                if qty > 0.0 {
                    *production_totals
                        .entry((merchant_id, settlement_id, good_id))
                        .or_insert(0.0) += qty;
                }
            }
        }

        // Update each merchant's production EMA once with total production this tick
        for ((merchant_id, settlement_id, good_id), total_qty) in production_totals {
            if let Some(merchant) = self.merchants.get_mut(&merchant_id) {
                merchant.record_production(settlement_id, good_id, total_qty);
            }
        }
    }

    /// Run mortality checks for all pops based on their food satisfaction.
    /// Pops may die (removed) or grow (spawn new pop).
    fn run_mortality_phase(&mut self) {
        use crate::mortality::{MortalityOutcome, check_mortality};

        // Skip mortality if no pops have food satisfaction tracked (no food need defined)
        let any_food_tracked = self
            .pops
            .values()
            .any(|p| p.need_satisfaction.contains_key("food"));
        if !any_food_tracked {
            return;
        }

        let mut rng = rand::rng();

        // Collect pop IDs and their outcomes
        let outcomes: Vec<(PopId, SettlementId, MortalityOutcome, f64)> = self
            .pops
            .iter()
            .map(|(id, pop)| {
                let food_satisfaction = pop.need_satisfaction.get("food").copied().unwrap_or(0.0);
                let outcome = check_mortality(&mut rng, food_satisfaction);
                (*id, pop.home_settlement, outcome, food_satisfaction)
            })
            .collect();

        #[cfg(feature = "instrument")]
        {
            let tick = self.tick;
            for (pop_id, settlement_id, outcome, food_satisfaction) in &outcomes {
                let outcome_str = match outcome {
                    MortalityOutcome::Dies => "dies",
                    MortalityOutcome::Grows => "grows",
                    MortalityOutcome::Survives => "survives",
                };
                let death_prob = crate::mortality::death_probability(*food_satisfaction);
                let growth_prob = crate::mortality::growth_probability(*food_satisfaction);
                tracing::info!(
                    target: "mortality",
                    tick = tick,
                    pop_id = pop_id.0,
                    settlement_id = settlement_id.0,
                    food_satisfaction = *food_satisfaction,
                    death_prob = death_prob,
                    growth_prob = growth_prob,
                    outcome = outcome_str,
                );
            }
        }

        // Process outcomes
        let mut new_pops: Vec<(SettlementId, Pop)> = Vec::new();
        let mut dead_pop_ids: Vec<PopId> = Vec::new();

        for (pop_id, settlement_id, outcome, _food_satisfaction) in outcomes {
            match outcome {
                MortalityOutcome::Dies => {
                    dead_pop_ids.push(pop_id);
                }
                MortalityOutcome::Grows => {
                    // Clone the pop to create a new one.
                    // Child startup funds come from the parent so growth doesn't mint currency.
                    if let Some(parent) = self.pops.get_mut(&pop_id) {
                        let mut child = parent.clone();
                        // Reset child's stocks to modest starting amount.
                        child.stocks.clear();

                        // Split parent savings with the child.
                        let child_currency = parent.currency * 0.5;
                        parent.currency -= child_currency;
                        child.currency = child_currency;

                        new_pops.push((settlement_id, child));
                    }
                }
                MortalityOutcome::Survives => {}
            }
        }

        // Remove dead pops
        for pop_id in &dead_pop_ids {
            if let Some(pop) = self.pops.remove(pop_id) {
                // Remove from settlement's pop list
                if let Some(settlement) = self.settlements.get_mut(&pop.home_settlement) {
                    settlement.pop_ids.retain(|id| id != pop_id);
                }
                // Remove from facility employment
                for facility in self.facilities.values_mut() {
                    // Note: Current design has pops as single workers, not tracked per-facility
                    // If pop was employed, reduce worker count
                    if pop.employed_at == Some(facility.id) {
                        for skill in &pop.skills {
                            if let Some(count) = facility.workers.get_mut(skill) {
                                *count = count.saturating_sub(1);
                            }
                        }
                    }
                }
            }
        }

        // Add new pops from growth
        for (settlement_id, child) in new_pops {
            if let Some(new_id) = self.add_pop(settlement_id)
                && let Some(new_pop) = self.pops.get_mut(&new_id)
            {
                // Copy over relevant fields from child
                new_pop.skills = child.skills;
                new_pop.min_wage = child.min_wage;
                new_pop.income_ema = child.income_ema;
                new_pop.currency = child.currency;
                new_pop.desired_consumption_ema = child.desired_consumption_ema;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_settlements_and_routes() {
        let mut world = World::new();

        let london = world.add_settlement("London", (0.0, 0.0));
        let paris = world.add_settlement("Paris", (100.0, 50.0));
        let amsterdam = world.add_settlement("Amsterdam", (50.0, 100.0));

        world.add_route(london, paris, 5);
        world.add_route(london, amsterdam, 3);

        assert_eq!(world.settlements.len(), 3);
        assert_eq!(world.routes.len(), 2);

        // Check connectivity
        let london_connections = world.connected_settlements(london);
        assert_eq!(london_connections.len(), 2);
        assert!(london_connections.contains(&paris));
        assert!(london_connections.contains(&amsterdam));

        // Paris only connects to London
        let paris_connections = world.connected_settlements(paris);
        assert_eq!(paris_connections.len(), 1);
        assert!(paris_connections.contains(&london));
    }

    #[test]
    fn test_pops() {
        let mut world = World::new();
        let london = world.add_settlement("London", (0.0, 0.0));

        // Add pops to london
        let pop1 = world.add_pop(london).unwrap();
        let pop2 = world.add_pop(london).unwrap();

        assert_eq!(world.pops.len(), 2);

        // Check settlement has both pops
        let settlement = world.get_settlement(london).unwrap();
        assert_eq!(settlement.pop_ids.len(), 2);

        // Check pops have correct home settlement
        assert_eq!(world.get_pop(pop1).unwrap().home_settlement, london);
        assert_eq!(world.get_pop(pop2).unwrap().home_settlement, london);

        // Check pops_at_settlement
        let pops = world.pops_at_settlement(london);
        assert_eq!(pops.len(), 2);
    }

    #[test]
    fn test_merchants() {
        let mut world = World::new();

        let m1 = world.add_merchant();
        let m2 = world.add_merchant();

        assert_eq!(world.merchants.len(), 2);
        assert!(world.get_merchant(m1).is_some());
        assert!(world.get_merchant(m2).is_some());
    }

    #[test]
    fn test_pop_stocks() {
        let mut world = World::new();
        let london = world.add_settlement("London", (0.0, 0.0));
        let pop_id = world.add_pop(london).unwrap();

        let grain: GoodId = 1;

        // Add stocks to pop
        let pop = world.get_pop_mut(pop_id).unwrap();
        pop.stocks.insert(grain, 100.0);

        // Retrieve
        let pop = world.get_pop(pop_id).unwrap();
        assert_eq!(*pop.stocks.get(&grain).unwrap(), 100.0);
    }

    #[test]
    fn test_facilities() {
        use crate::production::FacilityType;

        let mut world = World::new();
        let london = world.add_settlement("London", (0.0, 0.0));
        let paris = world.add_settlement("Paris", (100.0, 50.0));
        let merchant = world.add_merchant();

        // Add facilities
        let farm = world
            .add_facility(FacilityType::Farm, london, merchant)
            .unwrap();
        let bakery = world
            .add_facility(FacilityType::Bakery, london, merchant)
            .unwrap();
        let sawmill = world
            .add_facility(FacilityType::Sawmill, paris, merchant)
            .unwrap();

        assert_eq!(world.facilities.len(), 3);

        // Check facility properties
        let farm_facility = world.get_facility(farm).unwrap();
        assert_eq!(farm_facility.settlement, london);
        assert_eq!(farm_facility.owner, merchant);
        assert_eq!(farm_facility.facility_type, FacilityType::Farm);

        // Check merchant owns facilities
        let merchant_agent = world.get_merchant(merchant).unwrap();
        assert!(merchant_agent.facility_ids.contains(&farm));
        assert!(merchant_agent.facility_ids.contains(&bakery));
        assert!(merchant_agent.facility_ids.contains(&sawmill));

        // Check facilities at settlement
        let london_facilities = world.facilities_at_settlement(london);
        assert_eq!(london_facilities.len(), 2);

        let paris_facilities = world.facilities_at_settlement(paris);
        assert_eq!(paris_facilities.len(), 1);

        // Check merchant presence
        assert!(world.merchant_has_facility_at(merchant, london));
        assert!(world.merchant_has_facility_at(merchant, paris));

        // Check merchants at settlement
        let merchants_in_london = world.merchants_at_settlement(london);
        assert_eq!(merchants_in_london.len(), 1);
        assert!(merchants_in_london.contains(&merchant));
    }

    #[test]
    fn test_run_tick() {
        use crate::needs::Need;
        use crate::types::GoodProfile;

        let mut world = World::new();
        let london = world.add_settlement("London", (0.0, 0.0));

        // Add some pops
        let pop1 = world.add_pop(london).unwrap();
        let pop2 = world.add_pop(london).unwrap();

        // Give pops some currency and stocks
        let grain: GoodId = 1;
        {
            let pop = world.get_pop_mut(pop1).unwrap();
            pop.currency = 100.0;
            pop.stocks.insert(grain, 10.0);
        }
        {
            let pop = world.get_pop_mut(pop2).unwrap();
            pop.currency = 100.0;
            pop.stocks.insert(grain, 10.0);
        }

        // Set up good profiles, needs, and recipes
        let good_profiles = vec![GoodProfile {
            good: grain,
            contributions: vec![],
        }];
        let needs: std::collections::HashMap<String, Need> = std::collections::HashMap::new();
        let recipes: Vec<crate::production::Recipe> = vec![];

        // Run a tick
        assert_eq!(world.tick, 0);
        world.run_tick(&good_profiles, &needs, &recipes);
        assert_eq!(world.tick, 1);

        // Pops should still exist with their data
        assert!(world.get_pop(pop1).is_some());
        assert!(world.get_pop(pop2).is_some());
    }
}
