use std::collections::HashMap;

use crate::entities::{Population, Route, Ship};
use crate::market::{Ask, Bid};
use crate::state::{GameState, ShipOrder, get_recipe};
use crate::types::{
    EntityId, FacilityId, FacilityType, Good, KeyToU64, LocationId, OrgId, OrgType, SettlementId,
    ShipStatus,
};

// ============================================================================
// Constants for economic behavior
// ============================================================================

/// The subsistence wage - the economic anchor (20)
pub const SUBSISTENCE_WAGE: f32 = 20.0;

/// Labor force participation rate (100% of population works)
pub const LABOR_FORCE_RATE: f32 = 1.0;

/// How many ticks of inputs an org wants to keep on hand
pub const INPUT_TARGET_TICKS: f32 = 5.0;

/// How many ticks of outputs an org wants to hold before selling
pub const OUTPUT_TARGET_TICKS: f32 = 2.0;

// ============================================================================
// AI Decisions - Output of AI reasoning
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct AiDecisions {
    /// Ship orders: (ship_id_u64, order)
    pub ship_orders: Vec<(u64, ShipOrder)>,
    /// Facility priorities per settlement: settlement_id -> [facility_ids in priority order]
    pub facility_priorities: HashMap<u64, Vec<u64>>,
}

// ============================================================================
// AI Logic
// ============================================================================

/// Run AI decision-making for all orgs
/// Returns decisions without mutating state
pub fn run_org_ai(state: &GameState) -> AiDecisions {
    let mut decisions = AiDecisions::default();

    // For each org, make decisions
    for (org_id, _org) in state.orgs.iter() {
        let org_id_u64 = org_id.to_u64();

        // Generate ship orders for this org
        let ship_orders = generate_ship_orders(state, org_id_u64);
        decisions.ship_orders.extend(ship_orders);

        // Generate facility priorities for settlements where this org has facilities
        let priorities = generate_facility_priorities(state, org_id_u64);
        for (settlement_id, priority_list) in priorities {
            decisions
                .facility_priorities
                .insert(settlement_id, priority_list);
        }
    }

    decisions
}

/// Generate ship orders using global supply/demand matching
fn generate_ship_orders(state: &GameState, org_id: u64) -> Vec<(u64, ShipOrder)> {
    let mut orders = Vec::new();

    // Find all trade opportunities in the economy (source → sink for each good)
    let opportunities = find_global_trade_opportunities(state, org_id);

    // Find ships owned by this org that are in port
    let idle_ships: Vec<_> = state
        .ships
        .iter()
        .filter(|(_, ship)| ship.owner == org_id && ship.status == ShipStatus::InPort)
        .collect();

    // Assign ships to opportunities
    for (ship_id, ship) in idle_ships {
        if let Some(order) = assign_ship_to_trade(state, ship, org_id, &opportunities) {
            orders.push((ship_id.to_u64(), order));
        }
    }

    orders
}

/// A trade opportunity: move goods from source (cheap) to sink (expensive)
#[derive(Debug, Clone)]
struct TradeOpportunity {
    good: Good,
    source_id: u64, // Settlement with surplus (low price)
    sink_id: u64,   // Settlement with deficit (high price)
    #[allow(dead_code)]
    source_price: f32, // Kept for debugging
    #[allow(dead_code)]
    sink_price: f32, // Kept for debugging
    available: f32, // Goods available at source
    score: f32,     // Profitability score
}

/// Find all profitable trade routes in the economy
fn find_global_trade_opportunities(state: &GameState, org_id: u64) -> Vec<TradeOpportunity> {
    let mut opportunities = Vec::new();
    let org_id_key = OrgId::from(slotmap::KeyData::from_ffi(org_id));

    // For each good, find price differentials between settlements
    for good in Good::all() {
        // Collect (settlement_id, price, available) for this good
        let mut market_data: Vec<(u64, f32, f32)> = Vec::new();

        for (settlement_id, _settlement) in state.settlements.iter() {
            let price = state.get_market_price(settlement_id, good);

            // Check how much is available in our stockpile
            let available = state
                .get_stockpile(org_id_key, LocationId::from_settlement(settlement_id))
                .map(|s| s.get(good))
                .unwrap_or(0.0);

            market_data.push((settlement_id.to_u64(), price, available));
        }

        // Find source (lowest price with stock) and sink (highest price)
        let source = market_data
            .iter()
            .filter(|(_, _, avail)| *avail > 5.0) // Must have goods to sell
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let sink = market_data
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        if let (Some(source), Some(sink)) = (source, sink) {
            // Only create opportunity if there's meaningful price difference
            let price_ratio = sink.1 / source.1;
            if price_ratio > 1.2 && source.0 != sink.0 {
                // Check if route exists between source and sink
                let route_exists = state.routes.iter().any(|r| {
                    (r.from == source.0 && r.to == sink.0) || (r.from == sink.0 && r.to == source.0)
                });

                if route_exists {
                    let score = (sink.1 - source.1) * source.2.min(100.0); // Profit potential
                    opportunities.push(TradeOpportunity {
                        good,
                        source_id: source.0,
                        sink_id: sink.0,
                        source_price: source.1,
                        sink_price: sink.1,
                        available: source.2,
                        score,
                    });
                }
            }
        }
    }

    // Sort by score (best opportunities first)
    opportunities.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    opportunities
}

/// Assign a ship to the best available trade
fn assign_ship_to_trade(
    state: &GameState,
    ship: &Ship,
    org_id: u64,
    opportunities: &[TradeOpportunity],
) -> Option<ShipOrder> {
    let current_location = ship.location;

    // Debug: print ship location and opportunities
    let loc_name = state
        .settlements
        .iter()
        .find(|(id, _)| id.to_u64() == current_location)
        .map(|(_, s)| s.name.as_str())
        .unwrap_or("?");
    eprintln!(
        "  AI: Ship {} at {} evaluating {} opportunities",
        ship.name,
        loc_name,
        opportunities.len()
    );
    for opp in opportunities.iter().take(3) {
        let src_name = state
            .settlements
            .iter()
            .find(|(id, _)| id.to_u64() == opp.source_id)
            .map(|(_, s)| s.name.as_str())
            .unwrap_or("?");
        let sink_name = state
            .settlements
            .iter()
            .find(|(id, _)| id.to_u64() == opp.sink_id)
            .map(|(_, s)| s.name.as_str())
            .unwrap_or("?");
        eprintln!(
            "    {:?}: {} (p={:.1}) -> {} (p={:.1}), avail={:.1}",
            opp.good, src_name, opp.source_price, sink_name, opp.sink_price, opp.available
        );
    }

    // Find the best opportunity overall (first in sorted list)
    let best_opp = opportunities.first();

    // Find the best LOCAL opportunity (where we're at the source)
    let local_opp = opportunities
        .iter()
        .find(|opp| current_location == opp.source_id);

    // Decide: trade locally or go to best opportunity?
    // Only trade locally if it's at least 50% as good as the best opportunity
    if let (Some(local), Some(best)) = (local_opp, best_opp) {
        if local.score >= best.score * 0.5 {
            // Local trade is good enough, do it
            let amount = local.available.min(ship.capacity);
            eprintln!(
                "    -> LOCAL TRADE {:?}, loading {:.1} (score {:.0} vs best {:.0})",
                local.good, amount, local.score, best.score
            );
            if amount > 1.0 {
                return Some(ShipOrder {
                    destination: local.sink_id,
                    cargo: vec![(local.good, amount)],
                });
            }
        }
    }

    // Go to the best opportunity's source
    if let Some(best) = best_opp {
        // Check if there's a back-haul opportunity
        let backhaul = find_backhaul(
            state,
            org_id,
            current_location,
            best.source_id,
            ship.capacity,
        );
        eprintln!(
            "    -> GO TO SOURCE for {:?} at {}, backhaul: {:?}",
            best.good,
            best.source_id,
            backhaul.len()
        );

        return Some(ShipOrder {
            destination: best.source_id,
            cargo: backhaul,
        });
    }

    // No global opportunities - try any local export
    find_any_local_export(state, ship, org_id)
}

/// Find goods to carry on an otherwise empty leg (backhaul)
fn find_backhaul(
    state: &GameState,
    org_id: u64,
    from_id: u64,
    to_id: u64,
    capacity: f32,
) -> Vec<(Good, f32)> {
    let from_settlement_id = SettlementId::from(slotmap::KeyData::from_ffi(from_id));
    let to_settlement_id = SettlementId::from(slotmap::KeyData::from_ffi(to_id));
    let org_id_key = OrgId::from(slotmap::KeyData::from_ffi(org_id));

    if state.settlements.get(from_settlement_id).is_none() {
        return vec![];
    }
    if state.settlements.get(to_settlement_id).is_none() {
        return vec![];
    }

    let stockpile =
        match state.get_stockpile(org_id_key, LocationId::from_settlement(from_settlement_id)) {
            Some(s) => s,
            None => return vec![],
        };

    let mut cargo = Vec::new();
    let mut remaining_capacity = capacity;

    // Find goods that are worth carrying (even small margins count for backhaul)
    for good in Good::all() {
        let available = stockpile.get(good);
        if available < 1.0 {
            continue;
        }

        let from_price = state.get_market_price(from_settlement_id, good);
        let to_price = state.get_market_price(to_settlement_id, good);

        // Carry if destination price is higher (any profit on backhaul is good)
        if to_price > from_price {
            let amount = available.min(remaining_capacity);
            if amount > 0.5 {
                cargo.push((good, amount));
                remaining_capacity -= amount;
                if remaining_capacity < 1.0 {
                    break;
                }
            }
        }
    }

    cargo
}

/// Fallback: find any worthwhile local export
fn find_any_local_export(state: &GameState, ship: &Ship, org_id: u64) -> Option<ShipOrder> {
    let current_settlement_id = SettlementId::from(slotmap::KeyData::from_ffi(ship.location));
    let org_id_key = OrgId::from(slotmap::KeyData::from_ffi(org_id));

    state.settlements.get(current_settlement_id)?;
    let stockpile = state.get_stockpile(
        org_id_key,
        LocationId::from_settlement(current_settlement_id),
    )?;

    let mut best: Option<(u64, Good, f32, f32)> = None;

    for good in Good::all() {
        let available = stockpile.get(good);
        if available < 1.0 {
            continue;
        }

        let local_price = state.get_market_price(current_settlement_id, good);

        for route in &state.routes {
            let dest_id = if route.from == ship.location {
                route.to
            } else if route.to == ship.location {
                route.from
            } else {
                continue;
            };

            let dest_settlement_id = SettlementId::from(slotmap::KeyData::from_ffi(dest_id));
            if state.settlements.get(dest_settlement_id).is_some() {
                let dest_price = state.get_market_price(dest_settlement_id, good);

                let margin = dest_price / local_price;
                if margin > 1.1 {
                    let dominated = best
                        .as_ref()
                        .map(|(_, _, m, _)| margin <= *m)
                        .unwrap_or(false);
                    if !dominated {
                        best = Some((dest_id, good, margin, available.min(ship.capacity)));
                    }
                }
            }
        }
    }

    best.map(|(dest, good, _, amount)| ShipOrder {
        destination: dest,
        cargo: vec![(good, amount)],
    })
}

/// Generate facility priorities for labor allocation
/// Returns: settlement_id -> [facility_ids in priority order]
fn generate_facility_priorities(state: &GameState, org_id: u64) -> HashMap<u64, Vec<u64>> {
    let mut priorities: HashMap<u64, Vec<u64>> = HashMap::new();

    // Group facilities by settlement
    for (facility_id, facility) in state.facilities.iter() {
        if facility.owner != org_id {
            continue;
        }

        priorities
            .entry(facility.location)
            .or_default()
            .push(facility_id.to_u64());
    }

    // Sort each settlement's facilities by priority
    for (_settlement_id, facility_ids) in priorities.iter_mut() {
        facility_ids.sort_by_key(|fid| {
            let fid_key = slotmap::KeyData::from_ffi(*fid);
            let facility_id = FacilityId::from(fid_key);
            let facility = state.facilities.get(facility_id);

            match facility.map(|f| f.kind) {
                Some(FacilityType::Bakery) => 0,  // Highest priority - provisions
                Some(FacilityType::Mill) => 1,    // Second - flour for bakeries
                Some(FacilityType::Farm) => 2,    // Third - grain for mills
                Some(FacilityType::Fishery) => 2, // Third - fish for bakeries
                Some(FacilityType::SubsistenceFarm) => 100, // Not prioritized - fallback only
                None => 99,
            }
        });
    }

    priorities
}

// ============================================================================
// Helper to find routes (duplicated from lib.rs for now)
// ============================================================================

#[allow(dead_code)]
fn find_route(from: u64, to: u64, routes: &[Route]) -> Option<&Route> {
    routes
        .iter()
        .find(|r| (r.from == from && r.to == to) || (r.from == to && r.to == from))
}

// ============================================================================
// v2: Bid/Ask Generation for Unified Auctions
// ============================================================================

/// Generate labor asks from population (selling their labor)
pub fn generate_population_labor_asks(
    population: &Population,
    settlement_id: SettlementId,
) -> Vec<Ask> {
    let labor_pool = population.count as f32 * LABOR_FORCE_RATE;
    let wealth_ratio = population.wealth / population.target_wealth.max(1.0);

    // Wealth factor affects reservation wages
    let wealth_factor = if wealth_ratio > 1.5 {
        1.5 // Wealthy - demand premium
    } else if wealth_ratio > 1.0 {
        1.2 // Comfortable
    } else if wealth_ratio > 0.5 {
        1.0 // Normal
    } else {
        0.8 // Poor - accept less
    };

    // Generate asks at different price levels (10 buckets)
    (1..=10)
        .map(|p| {
            let percentile = p as f32 / 10.0;
            Ask {
                seller: EntityId::from_population(settlement_id),
                good: Good::Labor,
                quantity: labor_pool * 0.1, // 10% of labor pool per bucket
                min_price: SUBSISTENCE_WAGE * (1.0 + wealth_factor * percentile.sqrt()),
            }
        })
        .collect()
}

/// Generate goods bids from population (buying provisions and cloth)
///
/// Uses tight clustering around last clearing price.
/// Each bucket: price * qty = C = Budget / N_buckets
/// Higher prices get less quantity, lower prices get more.
pub fn generate_population_goods_bids(
    population: &Population,
    settlement_id: SettlementId,
    provisions_price: f32, // Last clearing price
) -> Vec<Bid> {
    let mut bids = Vec::new();

    // Stock ratio determines urgency (shifts price range up/down)
    let provisions_stock_ratio =
        population.stockpile_provisions / population.target_provisions.max(1.0);

    // Budget based on wealth
    let provisions_budget = population.wealth * 0.3;
    let n_buckets = 5;

    // Urgency shifts price range: low stock = willing to pay more
    let urgency = (1.0 - provisions_stock_ratio).clamp(0.0, 1.0);
    let center_price = provisions_price * (1.0 + urgency * 0.3);
    let spread = 0.2 + urgency * 0.1;

    for i in 0..n_buckets {
        // Price from high to low (eager buyers bid high first)
        let t = i as f32 / (n_buckets - 1) as f32;
        let price = center_price * (1.0 + spread - t * 2.0 * spread);
        let quantity = provisions_budget / price.max(0.01); // Full budget at this price

        if quantity > 0.1 && price > 0.1 {
            bids.push(Bid {
                buyer: EntityId::from_population(settlement_id),
                good: Good::Provisions,
                quantity,
                max_price: price,
            });
        }
    }

    bids
}

/// Generate labor bids from a facility (buying labor based on MVP)
pub fn generate_facility_labor_bids(
    state: &GameState,
    facility_id: FacilityId,
    org_id: OrgId,
    settlement_id: SettlementId,
) -> Vec<Bid> {
    let facility = match state.facilities.get(facility_id) {
        Some(f) => f,
        None => return vec![],
    };

    // Skip subsistence farms - they're handled separately
    if facility.kind == FacilityType::SubsistenceFarm {
        return vec![];
    }

    let recipe = get_recipe(facility.kind);

    // Check input availability from org's stockpile
    let stockpile = state.get_stockpile(org_id, LocationId::from_settlement(settlement_id));
    let gross_output = recipe.total_output(
        facility.optimal_workforce as f32,
        facility.optimal_workforce,
    );
    let input_eff = if recipe.inputs.is_empty() {
        1.0 // Extraction facilities have no inputs
    } else {
        recipe
            .inputs
            .iter()
            .map(|(good, ratio)| {
                let needed = gross_output * ratio;
                let available = stockpile.map(|s| s.get(*good)).unwrap_or(0.0);
                (available / needed.max(0.01)).min(1.0)
            })
            .fold(f32::MAX, f32::min)
    };

    if input_eff < 0.01 {
        return vec![]; // No inputs, no labor demand
    }

    // Calculate MVP using marginal output at current staffing level
    let output_price = state.get_market_price(settlement_id, recipe.output);
    let input_cost: f32 = recipe
        .inputs
        .iter()
        .map(|(g, r)| state.get_market_price(settlement_id, *g) * r)
        .sum();

    let effective_workers = facility.optimal_workforce as f32 * input_eff;
    let marginal = recipe.marginal_output(
        facility.current_workforce as f32,
        facility.optimal_workforce,
    );
    let mvp = marginal * (output_price - input_cost);

    // Only bid if profitable above subsistence
    if mvp <= SUBSISTENCE_WAGE {
        return vec![];
    }

    // Bid for workers in batches (25% each)
    let batch_size = effective_workers * 0.25;
    (0..4)
        .filter_map(|batch| {
            // Apply diminishing returns for batches beyond optimal
            let batch_end = (batch + 1) as f32 * 0.25;
            let diminishing = if batch_end > 1.0 {
                0.7_f32.powf(batch_end - 1.0)
            } else {
                1.0
            };
            let adjusted_mvp = mvp * diminishing;

            if adjusted_mvp > SUBSISTENCE_WAGE {
                Some(Bid {
                    buyer: EntityId::from_org(org_id),
                    good: Good::Labor,
                    quantity: batch_size,
                    max_price: adjusted_mvp,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Generate input bids from an org (buying facility inputs)
pub fn generate_org_input_bids(
    state: &GameState,
    org_id: OrgId,
    settlement_id: SettlementId,
) -> Vec<Bid> {
    let mut bids = Vec::new();

    let org = match state.orgs.get(org_id) {
        Some(o) => o,
        None => return vec![],
    };

    // Skip settlement orgs - they don't buy inputs
    if org.org_type == OrgType::Settlement {
        return vec![];
    }

    let treasury_ratio = org.treasury / 10000.0; // Assume 10k is "comfortable"

    let stockpile = state.get_stockpile(org_id, LocationId::from_settlement(settlement_id));

    // Find all facilities owned by this org at this settlement
    for (_fac_id, facility) in state.facilities.iter() {
        if facility.owner != org_id.to_u64() {
            continue;
        }
        if facility.location != settlement_id.to_u64() {
            continue;
        }
        if facility.kind == FacilityType::SubsistenceFarm {
            continue;
        }

        let recipe = get_recipe(facility.kind);

        for (input_good, ratio) in &recipe.inputs {
            let current = stockpile.map(|s| s.get(*input_good)).unwrap_or(0.0);
            let full_output = recipe.total_output(
                facility.optimal_workforce as f32,
                facility.optimal_workforce,
            );
            let target = full_output * ratio * INPUT_TARGET_TICKS;
            let shortfall = target - current;

            if shortfall <= 0.0 {
                continue;
            }

            let shortfall_ratio = shortfall / target.max(1.0);

            // Buy urgency based on treasury and shortfall
            let urgency = if treasury_ratio > 1.5 {
                shortfall_ratio * 1.0
            } else if treasury_ratio > 0.8 {
                shortfall_ratio * 0.7
            } else if treasury_ratio > 0.3 {
                shortfall_ratio * 0.4
            } else {
                shortfall_ratio * 0.1
            };

            // Max price multiplier
            let max_mult = if treasury_ratio > 1.0 && shortfall_ratio > 0.8 {
                1.5 // Desperate and rich
            } else if shortfall_ratio > 0.5 {
                1.2
            } else {
                1.0
            };

            let market_price = state.get_market_price(settlement_id, *input_good);

            if urgency > 0.01 {
                bids.push(Bid {
                    buyer: EntityId::from_org(org_id),
                    good: *input_good,
                    quantity: shortfall * urgency,
                    max_price: market_price * max_mult,
                });
            }
        }
    }

    bids
}

/// Generate output asks from an org (selling facility outputs)
pub fn generate_org_output_asks(
    state: &GameState,
    org_id: OrgId,
    settlement_id: SettlementId,
) -> Vec<Ask> {
    let mut asks = Vec::new();

    let org = match state.orgs.get(org_id) {
        Some(o) => o,
        None => return vec![],
    };

    // Skip settlement orgs - handled separately
    if org.org_type == OrgType::Settlement {
        return vec![];
    }

    let treasury_ratio = org.treasury / 10000.0;

    let stockpile = state.get_stockpile(org_id, LocationId::from_settlement(settlement_id));

    // Find all facilities owned by this org at this settlement
    for (_fac_id, facility) in state.facilities.iter() {
        if facility.owner != org_id.to_u64() {
            continue;
        }
        if facility.location != settlement_id.to_u64() {
            continue;
        }
        if facility.kind == FacilityType::SubsistenceFarm {
            continue;
        }

        let recipe = get_recipe(facility.kind);
        let output_good = recipe.output;

        let current = stockpile.map(|s| s.get(output_good)).unwrap_or(0.0);
        let full_output = recipe.total_output(
            facility.optimal_workforce as f32,
            facility.optimal_workforce,
        );
        let target = full_output * OUTPUT_TARGET_TICKS;
        let surplus = current - target;

        if surplus <= 0.0 {
            continue;
        }

        let surplus_ratio = surplus / target.max(1.0);

        // Sell urgency
        let urgency = if treasury_ratio < 0.3 {
            1.0 // Liquidate - need cash
        } else if treasury_ratio < 0.8 {
            0.8
        } else if surplus_ratio > 1.0 {
            0.7 // Clear excess
        } else {
            0.4
        };

        // Min price multiplier
        let min_mult = if treasury_ratio < 0.3 {
            0.7 // Desperate
        } else if surplus_ratio > 2.0 {
            0.8 // Need to clear
        } else {
            0.95 // Hold for fair price
        };

        let market_price = state.get_market_price(settlement_id, output_good);

        if urgency > 0.01 {
            asks.push(Ask {
                seller: EntityId::from_org(org_id),
                good: output_good,
                quantity: surplus * urgency,
                min_price: market_price * min_mult,
            });
        }
    }

    asks
}

/// Generate asks from ships at port (selling cargo)
pub fn generate_ship_goods_asks(
    state: &GameState,
    ship_id: crate::types::ShipId,
    settlement_id: SettlementId,
) -> Vec<Ask> {
    let ship = match state.ships.get(ship_id) {
        Some(s) => s,
        None => return vec![],
    };

    // Only trade if in port at this settlement
    if ship.status != ShipStatus::InPort || ship.location != settlement_id.to_u64() {
        return vec![];
    }

    let org_id = OrgId::from(slotmap::KeyData::from_ffi(ship.owner));
    let ship_stockpile = match state.get_stockpile(org_id, LocationId::from_ship(ship_id)) {
        Some(s) => s,
        None => return vec![],
    };

    let mut asks = Vec::new();

    // Sell cargo that fetches good prices here
    for (good, qty) in &ship_stockpile.goods {
        if *qty < 0.1 {
            continue;
        }

        let local_price = state.get_market_price(settlement_id, *good);

        // Check if this is a high-price location by comparing to other settlements
        let avg_price: f32 = state
            .settlements
            .keys()
            .map(|sid| state.get_market_price(sid, *good))
            .sum::<f32>()
            / state.settlements.len().max(1) as f32;

        // Sell if local price is above average (we're at a good selling point)
        if local_price >= avg_price * 0.95 {
            asks.push(Ask {
                seller: EntityId::from_ship(ship_id),
                good: *good,
                quantity: *qty,
                min_price: local_price * 0.95, // Willing to sell slightly below market
            });
        }
    }

    asks
}

/// Generate bids from ships at port (buying cargo)
pub fn generate_ship_goods_bids(
    state: &GameState,
    ship_id: crate::types::ShipId,
    settlement_id: SettlementId,
) -> Vec<Bid> {
    let ship = match state.ships.get(ship_id) {
        Some(s) => s,
        None => return vec![],
    };

    // Only trade if in port at this settlement
    if ship.status != ShipStatus::InPort || ship.location != settlement_id.to_u64() {
        return vec![];
    }

    let org_id = OrgId::from(slotmap::KeyData::from_ffi(ship.owner));
    let org = match state.orgs.get(org_id) {
        Some(o) => o,
        None => return vec![],
    };

    // Calculate remaining capacity from ship's stockpile
    let current_cargo: f32 = state
        .get_stockpile(org_id, LocationId::from_ship(ship_id))
        .map(|s| s.total())
        .unwrap_or(0.0);
    let remaining_capacity = ship.capacity - current_cargo;
    if remaining_capacity < 1.0 {
        return vec![];
    }

    let mut bids = Vec::new();
    let budget = org.treasury * 0.2; // Limit spending per port

    // Buy goods that are cheap here
    for good in Good::physical() {
        let local_price = state.get_market_price(settlement_id, good);

        // Check if this is a low-price location
        let avg_price: f32 = state
            .settlements
            .keys()
            .map(|sid| state.get_market_price(sid, good))
            .sum::<f32>()
            / state.settlements.len().max(1) as f32;

        // Buy if local price is below average (good buying opportunity)
        if local_price <= avg_price * 1.05 {
            let qty = (remaining_capacity).min(budget / local_price.max(0.01));
            if qty > 0.5 {
                bids.push(Bid {
                    buyer: EntityId::from_ship(ship_id),
                    good,
                    quantity: qty,
                    max_price: local_price * 1.05, // Willing to pay slightly above market
                });
            }
        }
    }

    bids
}

/// Generate asks from settlement org (selling subsistence output)
///
/// Uses tight clustering around last clearing price.
/// Each bucket offers equal quantity, prices spread around market price.
/// High stock → lower prices, low stock → higher prices.
pub fn generate_settlement_org_asks(state: &GameState, settlement_id: SettlementId) -> Vec<Ask> {
    let settlement = match state.settlements.get(settlement_id) {
        Some(s) => s,
        None => return vec![],
    };

    let org_id_u64 = match settlement.org_id {
        Some(id) => id,
        None => return vec![],
    };

    let org_id = OrgId::from(slotmap::KeyData::from_ffi(org_id_u64));

    let stockpile = state.get_stockpile(org_id, LocationId::from_settlement(settlement_id));
    let provisions = stockpile.map(|s| s.get(Good::Provisions)).unwrap_or(0.0);

    if provisions < 0.1 {
        return vec![];
    }

    // Get last clearing price
    let market_price = state.get_market_price(settlement_id, Good::Provisions);

    // Stock ratio determines eagerness
    let target_stock = settlement.population.count as f32 * 2.0;
    let stock_ratio = provisions / target_stock.max(1.0);

    // Settlement orgs always want to sell — they exist to feed people, not profit.
    // Eagerness scales with stock: more stock = lower prices to ensure clearing.
    let eagerness = stock_ratio.clamp(0.1, 1.5);
    let center_price = market_price * (1.0 - eagerness * 0.1); // Always slightly below market
    let spread = 0.1 + eagerness * 0.05; // Tight spread to maximize clearing

    let n_buckets = 5;

    let mut asks = Vec::new();

    // Each bucket offers the FULL quantity at that price.
    // Stock constraint is enforced during trade execution.
    for i in 0..n_buckets {
        let t = i as f32 / (n_buckets - 1) as f32;
        let price = center_price * (1.0 - spread + t * 2.0 * spread); // low to high

        if provisions > 0.1 && price > 0.1 {
            asks.push(Ask {
                seller: EntityId::from_org(org_id),
                good: Good::Provisions,
                quantity: provisions, // Full stock at this price
                min_price: price,
            });
        }
    }

    asks
}
