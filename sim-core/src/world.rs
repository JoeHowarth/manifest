// World state for the pops economic simulation

use std::collections::HashMap;

use crate::agents::{MerchantAgent, Pop, Stockpile};
use crate::geography::{Route, Settlement};
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

    // ID counters
    next_settlement_id: u32,
    next_pop_id: u32,
    next_merchant_id: u32,
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
            next_settlement_id: 0,
            next_pop_id: 0,
            next_merchant_id: 0,
            next_facility_id: 0,
        }
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

        let id = PopId::new(self.next_pop_id);
        self.next_pop_id += 1;

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
        let id = MerchantId::new(self.next_merchant_id);
        self.next_merchant_id += 1;

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

            let _result = execute_production(&allocation, recipes, stockpile, quality_multiplier);
        }
    }

    // === Simulation Tick ===

    /// Run one simulation tick across all settlements.
    ///
    /// Tick phases:
    /// 0. Production - facilities produce goods using workers and inputs
    /// 1. Consumption - pops consume goods to satisfy needs
    /// 2. Market clearing - call auction for each settlement
    /// 3. Price EMA update
    pub fn run_tick(
        &mut self,
        good_profiles: &[crate::types::GoodProfile],
        needs: &std::collections::HashMap<String, crate::needs::Need>,
        recipes: &[Recipe],
    ) {
        use crate::tick::run_settlement_tick;

        self.tick += 1;

        // === 0. PRODUCTION PHASE ===
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
