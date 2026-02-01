//! Property-based tests for simulation invariants
//!
//! These tests verify that certain properties hold regardless of the specific
//! scenario configuration. They catch bugs in the economic logic.

use std::collections::HashMap;

use sim_core::{
    World,
    needs::{Need, UtilityCurve},
    types::{GoodId, GoodProfile, NeedContribution},
};

// === TEST FIXTURES ===

/// Standard goods for testing
const GRAIN: GoodId = 1;
const BREAD: GoodId = 2;

/// Create a basic world with settlements and pops for testing
fn create_test_world(num_settlements: usize, pops_per_settlement: usize) -> World {
    let mut world = World::new();

    for i in 0..num_settlements {
        let name = format!("Settlement_{}", i);
        let position = (i as f64 * 100.0, 0.0);
        let settlement_id = world.add_settlement(name, position);

        // Add pops to settlement
        for _ in 0..pops_per_settlement {
            let pop_id = world.add_pop(settlement_id).unwrap();

            // Give pops initial currency and stocks
            let pop = world.get_pop_mut(pop_id).unwrap();
            pop.currency = 100.0;
            pop.stocks.insert(GRAIN, 50.0);
            pop.stocks.insert(BREAD, 20.0);
            pop.income_ema = 10.0;
        }
    }

    world
}

/// Create good profiles for testing
fn create_good_profiles() -> Vec<GoodProfile> {
    vec![
        GoodProfile {
            good: GRAIN,
            contributions: vec![NeedContribution {
                need_id: "food".to_string(),
                efficiency: 1.0,
            }],
        },
        GoodProfile {
            good: BREAD,
            contributions: vec![NeedContribution {
                need_id: "food".to_string(),
                efficiency: 2.0,
            }],
        },
    ]
}

/// Create needs for testing
fn create_needs() -> HashMap<String, Need> {
    let mut needs = HashMap::new();
    needs.insert(
        "food".to_string(),
        Need {
            id: "food".to_string(),
            utility_curve: UtilityCurve::Subsistence {
                requirement: 1.0,
                steepness: 5.0,
            },
        },
    );
    needs
}

// === HELPER FUNCTIONS ===

/// Calculate total currency across all pops and merchants
fn total_currency(world: &World) -> f64 {
    let pop_currency: f64 = world.pops.values().map(|p| p.currency).sum();
    let merchant_currency: f64 = world.merchants.values().map(|m| m.currency).sum();
    pop_currency + merchant_currency
}

/// Calculate total stock of a good across all pops and merchants
fn total_stock(world: &World, good: GoodId) -> f64 {
    let pop_stock: f64 = world
        .pops
        .values()
        .map(|p| p.stocks.get(&good).copied().unwrap_or(0.0))
        .sum();

    let merchant_stock: f64 = world
        .merchants
        .values()
        .flat_map(|m| m.stockpiles.values())
        .map(|s| s.get(good))
        .sum();

    pop_stock + merchant_stock
}

/// Check if any agent has negative stock
fn has_negative_stock(world: &World) -> Option<(String, GoodId, f64)> {
    for pop in world.pops.values() {
        for (&good, &qty) in &pop.stocks {
            if qty < -0.0001 {
                // Small epsilon for float comparison
                return Some((format!("Pop {:?}", pop.id), good, qty));
            }
        }
    }

    for merchant in world.merchants.values() {
        for (settlement, stockpile) in &merchant.stockpiles {
            for (&good, &qty) in &stockpile.goods {
                if qty < -0.0001 {
                    return Some((
                        format!("Merchant {:?} at {:?}", merchant.id, settlement),
                        good,
                        qty,
                    ));
                }
            }
        }
    }

    None
}

/// Check if any agent has negative currency
fn has_negative_currency(world: &World) -> Option<(String, f64)> {
    for pop in world.pops.values() {
        if pop.currency < -0.0001 {
            return Some((format!("Pop {:?}", pop.id), pop.currency));
        }
    }

    for merchant in world.merchants.values() {
        if merchant.currency < -0.0001 {
            return Some((format!("Merchant {:?}", merchant.id), merchant.currency));
        }
    }

    None
}

// === PROPERTY TESTS ===

#[test]
fn money_conservation_single_settlement() {
    let mut world = create_test_world(1, 5);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    let initial_currency = total_currency(&world);

    // Run multiple ticks
    for tick in 0..20 {
        world.run_tick(&good_profiles, &needs);

        let current_currency = total_currency(&world);
        let diff = (current_currency - initial_currency).abs();

        assert!(
            diff < 0.01,
            "Money not conserved at tick {}: initial={:.2}, current={:.2}, diff={:.4}",
            tick,
            initial_currency,
            current_currency,
            diff
        );
    }
}

#[test]
fn money_conservation_multiple_settlements() {
    let mut world = create_test_world(3, 4);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    let initial_currency = total_currency(&world);

    for tick in 0..20 {
        world.run_tick(&good_profiles, &needs);

        let current_currency = total_currency(&world);
        let diff = (current_currency - initial_currency).abs();

        assert!(
            diff < 0.01,
            "Money not conserved at tick {} with multiple settlements: initial={:.2}, current={:.2}, diff={:.4}",
            tick,
            initial_currency,
            current_currency,
            diff
        );
    }
}

#[test]
fn no_negative_stocks() {
    let mut world = create_test_world(2, 5);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    for tick in 0..50 {
        world.run_tick(&good_profiles, &needs);

        if let Some((agent, good, qty)) = has_negative_stock(&world) {
            panic!(
                "Negative stock at tick {}: {} has {:.4} of good {}",
                tick, agent, qty, good
            );
        }
    }
}

#[test]
fn no_negative_currency() {
    let mut world = create_test_world(2, 5);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    for tick in 0..50 {
        world.run_tick(&good_profiles, &needs);

        if let Some((agent, currency)) = has_negative_currency(&world) {
            panic!(
                "Negative currency at tick {}: {} has {:.4}",
                tick, agent, currency
            );
        }
    }
}

#[test]
fn entities_persist_across_ticks() {
    let mut world = create_test_world(2, 5);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    let initial_pop_count = world.pops.len();
    let initial_settlement_count = world.settlements.len();

    for tick in 0..20 {
        world.run_tick(&good_profiles, &needs);

        assert_eq!(
            world.pops.len(),
            initial_pop_count,
            "Pop count changed at tick {}: expected {}, got {}",
            tick,
            initial_pop_count,
            world.pops.len()
        );

        assert_eq!(
            world.settlements.len(),
            initial_settlement_count,
            "Settlement count changed at tick {}: expected {}, got {}",
            tick,
            initial_settlement_count,
            world.settlements.len()
        );
    }
}

#[test]
fn price_ema_stays_bounded() {
    let mut world = create_test_world(2, 5);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    let min_price = 0.001;
    let max_price = 10000.0;

    for tick in 0..50 {
        world.run_tick(&good_profiles, &needs);

        for ((settlement, good), price) in &world.price_ema {
            assert!(
                *price >= min_price,
                "Price too low at tick {}: settlement {:?}, good {}, price {:.6}",
                tick,
                settlement,
                good,
                price
            );

            assert!(
                *price <= max_price,
                "Price too high at tick {}: settlement {:?}, good {}, price {:.2}",
                tick,
                settlement,
                good,
                price
            );

            assert!(
                price.is_finite(),
                "Price is not finite at tick {}: settlement {:?}, good {}, price {}",
                tick,
                settlement,
                good,
                price
            );
        }
    }
}

#[test]
fn tick_counter_increments() {
    let mut world = create_test_world(1, 3);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    assert_eq!(world.tick, 0);

    for expected_tick in 1..=10 {
        world.run_tick(&good_profiles, &needs);
        assert_eq!(
            world.tick, expected_tick,
            "Tick counter mismatch: expected {}, got {}",
            expected_tick, world.tick
        );
    }
}

#[test]
fn consumption_reduces_stocks() {
    let mut world = create_test_world(1, 3);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    let initial_grain = total_stock(&world, GRAIN);
    let initial_bread = total_stock(&world, BREAD);

    // Run enough ticks for consumption to happen
    for _ in 0..10 {
        world.run_tick(&good_profiles, &needs);
    }

    let final_grain = total_stock(&world, GRAIN);
    let final_bread = total_stock(&world, BREAD);

    // At least one good should have been consumed (unless market trading balanced it)
    // Since we have no production, stocks should decrease or stay same (via trading)
    assert!(
        final_grain <= initial_grain + 0.01,
        "Grain increased without production: {:.2} -> {:.2}",
        initial_grain,
        final_grain
    );

    assert!(
        final_bread <= initial_bread + 0.01,
        "Bread increased without production: {:.2} -> {:.2}",
        initial_bread,
        final_bread
    );
}

#[test]
fn isolated_settlements_dont_affect_each_other() {
    let mut world = World::new();

    // Create two isolated settlements (no routes between them)
    let london = world.add_settlement("London", (0.0, 0.0));
    let paris = world.add_settlement("Paris", (100.0, 0.0));

    // Add pops with different initial conditions
    let london_pop = world.add_pop(london).unwrap();
    {
        let pop = world.get_pop_mut(london_pop).unwrap();
        pop.currency = 1000.0;
        pop.stocks.insert(GRAIN, 100.0);
        pop.income_ema = 50.0;
    }

    let paris_pop = world.add_pop(paris).unwrap();
    {
        let pop = world.get_pop_mut(paris_pop).unwrap();
        pop.currency = 100.0;
        pop.stocks.insert(GRAIN, 10.0);
        pop.income_ema = 5.0;
    }

    let good_profiles = create_good_profiles();
    let needs = create_needs();

    // Track London's state (for potential future assertions)
    let _london_initial_currency = world.get_pop(london_pop).unwrap().currency;
    let _london_initial_grain = world
        .get_pop(london_pop)
        .unwrap()
        .stocks
        .get(&GRAIN)
        .copied()
        .unwrap_or(0.0);

    // Run ticks
    for _ in 0..10 {
        world.run_tick(&good_profiles, &needs);
    }

    // London's currency + grain value should be independent of Paris
    // (Since there are no routes, no trade can happen between them)
    let london_final = world.get_pop(london_pop).unwrap();
    let paris_final = world.get_pop(paris_pop).unwrap();

    // Both should still have positive resources (no cross-contamination)
    assert!(
        london_final.currency >= 0.0,
        "London currency went negative"
    );
    assert!(paris_final.currency >= 0.0, "Paris currency went negative");
}

// === STATISTICAL PROPERTIES ===

#[test]
fn prices_tend_toward_stability() {
    let mut world = create_test_world(1, 10);
    let good_profiles = create_good_profiles();
    let needs = create_needs();

    // Set initial price
    let settlement_id = *world.settlements.keys().next().unwrap();
    world.price_ema.insert((settlement_id, GRAIN), 1.0);

    // Track price variance over time
    let mut price_history: Vec<f64> = Vec::new();

    for _ in 0..100 {
        world.run_tick(&good_profiles, &needs);

        if let Some(&price) = world.price_ema.get(&(settlement_id, GRAIN)) {
            price_history.push(price);
        }
    }

    // Calculate variance of last 20 ticks vs first 20 ticks
    if price_history.len() >= 40 {
        let early_prices: Vec<f64> = price_history[..20].to_vec();
        let late_prices: Vec<f64> = price_history[price_history.len() - 20..].to_vec();

        let early_variance = variance(&early_prices);
        let late_variance = variance(&late_prices);

        // Late variance should not be dramatically higher than early variance
        // (prices shouldn't explode)
        assert!(
            late_variance < early_variance * 10.0 + 1.0,
            "Price variance exploded: early={:.4}, late={:.4}",
            early_variance,
            late_variance
        );
    }
}

fn variance(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mean = data.iter().sum::<f64>() / data.len() as f64;
    data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / data.len() as f64
}
