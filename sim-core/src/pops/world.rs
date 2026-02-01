// World state for the pops economic simulation

use std::collections::HashMap;

use crate::pops::agents::{Stockpile, StockpileKey};
use crate::pops::geography::{Route, Settlement};
use crate::pops::types::{GoodId, LocationId, Price, SettlementId};

/// Complete state of the economic simulation
#[derive(Debug, Clone)]
pub struct World {
    pub tick: u64,

    // Geography
    pub settlements: HashMap<SettlementId, Settlement>,
    pub routes: Vec<Route>,

    // Distributed state
    pub stockpiles: HashMap<StockpileKey, Stockpile>,

    // Market state per settlement
    pub price_ema: HashMap<(SettlementId, GoodId), Price>,

    // ID counters
    next_settlement_id: u32,
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
            stockpiles: HashMap::new(),
            price_ema: HashMap::new(),
            next_settlement_id: 0,
        }
    }

    // === Settlement Management ===

    /// Add a settlement to the world, returns its ID
    pub fn add_settlement(&mut self, name: impl Into<String>, position: (f64, f64)) -> SettlementId {
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

    // === Stockpile Management ===

    /// Get or create a stockpile for an agent at a location
    pub fn get_stockpile_mut(&mut self, key: StockpileKey) -> &mut Stockpile {
        self.stockpiles.entry(key).or_default()
    }

    /// Get a stockpile for an agent at a location (read-only)
    pub fn get_stockpile(&self, key: StockpileKey) -> Option<&Stockpile> {
        self.stockpiles.get(&key)
    }

    /// Convenience: get stockpile at a settlement
    pub fn stockpile_at_settlement(
        &mut self,
        agent_id: crate::pops::types::AgentId,
        settlement_id: SettlementId,
    ) -> &mut Stockpile {
        let key = (agent_id, LocationId::settlement(settlement_id));
        self.get_stockpile_mut(key)
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
    fn test_stockpiles() {
        let mut world = World::new();
        let london = world.add_settlement("London", (0.0, 0.0));

        let agent_id = 1;
        let grain = 1;

        // Add goods to stockpile
        world.stockpile_at_settlement(agent_id, london).add(grain, 100.0);

        // Retrieve
        let key = (agent_id, LocationId::settlement(london));
        let stockpile = world.get_stockpile(key).unwrap();
        assert_eq!(stockpile.get(grain), 100.0);
    }
}
