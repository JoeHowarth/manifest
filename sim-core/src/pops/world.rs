// World state for the pops economic simulation

use std::collections::HashMap;

use crate::pops::agents::{MerchantAgent, Pop};
use crate::pops::geography::{Route, Settlement};
use crate::pops::types::{GoodId, MerchantId, PopId, Price, SettlementId};

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

    // Market state per settlement
    pub price_ema: HashMap<(SettlementId, GoodId), Price>,

    // ID counters
    next_settlement_id: u32,
    next_pop_id: u32,
    next_merchant_id: u32,
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
            price_ema: HashMap::new(),
            next_settlement_id: 0,
            next_pop_id: 0,
            next_merchant_id: 0,
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
}
