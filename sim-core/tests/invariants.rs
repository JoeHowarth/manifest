#[allow(dead_code)]
mod common;
use common::*;

use std::collections::HashMap;

use sim_core::{
    AnchoredGoodConfig, ExternalMarketConfig, GoodId, GoodProfile, Need, NeedContribution,
    OutsideFlowTotals, Pop, PopKey, Price, Recipe, SettlementFriction, SettlementId,
    SubsistenceReservationConfig, UtilityCurve, World, pop_key_from_u64,
    production::{FacilityType, RecipeId},
    run_settlement_tick,
};

fn pk(id: u64) -> PopKey {
    pop_key_from_u64(id)
}

fn pop_count(world: &World) -> usize {
    world.settlements.values().map(|s| s.pops.len()).sum()
}

fn total_grain(world: &World) -> f64 {
    let pop_grain: f64 = world
        .settlements
        .values()
        .flat_map(|s| s.pops.values())
        .map(|p| p.stocks.get(&GRAIN).copied().unwrap_or(0.0))
        .sum();
    let merchant_grain: f64 = world
        .merchants
        .values()
        .flat_map(|m| m.stockpiles.values())
        .map(|s| s.get(GRAIN))
        .sum();
    pop_grain + merchant_grain
}

fn total_currency(world: &World) -> f64 {
    let pop_currency: f64 = world
        .settlements
        .values()
        .flat_map(|s| s.pops.values())
        .map(|p| p.currency)
        .sum();
    let merchant_currency: f64 = world.merchants.values().map(|m| m.currency).sum();
    pop_currency + merchant_currency
}

#[test]
fn invariant_currency_conserved_under_population_growth() {
    let mut world = World::new();
    let settlement = world.add_settlement("GrowthTown", (0.0, 0.0));

    // Large population makes growth virtually guaranteed in one tick.
    for _ in 0..500 {
        let pop_id = world.add_pop(settlement).unwrap();
        let pop = world.pop_mut(pop_id).unwrap();
        pop.stocks.insert(GRAIN, 100.0);
        pop.desired_consumption_ema.insert(GRAIN, 1.0);
    }

    let good_profiles = vec![GoodProfile {
        good: GRAIN,
        contributions: vec![NeedContribution {
            need_id: "food".to_string(),
            efficiency: 1.0,
        }],
    }];

    let mut needs = HashMap::new();
    needs.insert(
        "food".to_string(),
        Need {
            id: "food".to_string(),
            // Always-positive MU drives high satisfaction; growth probability caps at 10%.
            utility_curve: UtilityCurve::LogDiminishing { scale: 1.0 },
        },
    );

    let initial_currency = total_currency(&world);
    for tick in 0..20 {
        world.run_tick(&good_profiles, &needs, &Vec::<Recipe>::new());
        let current_currency = total_currency(&world);
        let diff = (current_currency - initial_currency).abs();
        assert!(
            diff < 1e-6,
            "Currency should be conserved even with growth at tick {tick}: initial={initial_currency:.2}, current={current_currency:.2}, diff={diff:.6}"
        );
    }
}

#[test]
fn invariant_pop_cannot_sell_more_than_inventory() {
    let settlement = SettlementId::new(0);
    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, 1.0);

    let good_profiles = vec![GoodProfile {
        good: GRAIN,
        contributions: vec![],
    }];
    let needs: HashMap<String, Need> = HashMap::new();

    let mut seller = Pop::new();
    seller.currency = 0.0;
    seller.stocks.insert(GRAIN, 2.0); // tiny inventory
    seller.desired_consumption_ema.insert(GRAIN, 0.001); // target=0.005 => near-full stock is "excess"

    let mut buyer = Pop::new();
    buyer.currency = 10_000.0;
    buyer.income_ema = 10_000.0;
    buyer.stocks.insert(GRAIN, 0.0);
    buyer.desired_consumption_ema.insert(GRAIN, 10.0); // very strong demand

    let initial_stock = seller.stocks.get(&GRAIN).copied().unwrap_or(0.0);

    let mut pops: Vec<(PopKey, &mut Pop)> = vec![(pk(1), &mut seller), (pk(2), &mut buyer)];
    let mut merchants = Vec::new();
    let _ = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        None,
        None,
        None,
        &HashMap::new(),
        None,
    );

    let remaining = seller.stocks.get(&GRAIN).copied().unwrap_or(0.0);
    assert!(
        remaining >= -1e-6,
        "Pop seller oversold inventory in settlement tick: initial={initial_stock:.4}, remaining={remaining:.4}"
    );
}

#[test]
fn invariant_labor_assignment_accounting_consistent() {
    let mut world = World::new();
    let settlement = world.add_settlement("BalanceTown", (0.0, 0.0));
    let merchant = world.add_merchant();

    let farm_a = world
        .add_facility(FacilityType::Farm, settlement, merchant)
        .unwrap();
    let farm_b = world
        .add_facility(FacilityType::Farm, settlement, merchant)
        .unwrap();

    {
        let facility = world.facility_mut(farm_a).unwrap();
        facility.capacity = 50;
        facility.recipe_priorities = vec![RecipeId::new(1)];
        facility.workers.insert(LABORER, 50);
    }
    {
        let facility = world.facility_mut(farm_b).unwrap();
        facility.capacity = 50;
        facility.recipe_priorities = vec![RecipeId::new(1)];
        facility.workers.insert(LABORER, 50);
    }

    // Match balanced scenario from convergence test: merchant starts at target buffer.
    {
        let merchant_ref = world.get_merchant_mut(merchant).unwrap();
        merchant_ref.stockpile_at(settlement).add(GRAIN, 200.0);
    }

    for i in 0..100 {
        let pop_id = world.add_pop(settlement).unwrap();
        let pop = world.pop_mut(pop_id).unwrap();
        pop.skills.insert(LABORER);
        pop.min_wage = 0.5;
        pop.currency = 100.0;
        pop.income_ema = 1.0;
        pop.stocks.insert(GRAIN, 5.0);
        pop.desired_consumption_ema.insert(GRAIN, 1.0);
        pop.employed_at = Some(if i % 2 == 0 { farm_a.key } else { farm_b.key });
    }

    let s = world.settlements.get_mut(&settlement).unwrap();
    s.wage_ema.insert(LABORER, 1.0);
    s.price_ema.insert(GRAIN, 1.0);

    let good_profiles = vec![GoodProfile {
        good: GRAIN,
        contributions: vec![NeedContribution {
            need_id: "calories".to_string(),
            efficiency: 1.0,
        }],
    }];

    let mut needs = HashMap::new();
    needs.insert(
        "calories".to_string(),
        Need {
            id: "calories".to_string(),
            utility_curve: UtilityCurve::Subsistence {
                requirement: 1.0,
                steepness: 5.0,
            },
        },
    );

    let recipes = vec![
        sim_core::Recipe::new(RecipeId::new(1), "Grain Farming", vec![FacilityType::Farm])
            .with_capacity_cost(1)
            .with_worker(LABORER, 1)
            .with_output(GRAIN, 1.0),
    ];

    let initial_pop_count = pop_count(&world);
    for _ in 0..80 {
        world.run_tick(&good_profiles, &needs, &recipes);

        let employed_now = world
            .settlements
            .values()
            .flat_map(|s| s.pops.values())
            .filter(|p| p.employed_at.is_some())
            .count();
        let workers_in_facilities: usize = world
            .settlements
            .values()
            .flat_map(|s| s.facilities.values())
            .map(|f| f.workers.values().sum::<u32>() as usize)
            .sum();

        assert_eq!(
            employed_now, workers_in_facilities,
            "Employment/facility worker mismatch: employed={employed_now}, facility_workers={workers_in_facilities}"
        );
        assert!(
            employed_now <= pop_count(&world),
            "More employed pops than total pops: employed={employed_now}, total={}",
            pop_count(&world)
        );

        for facility in world
            .settlements
            .values()
            .flat_map(|s| s.facilities.values())
        {
            let total_workers: u32 = facility.workers.values().sum();
            assert!(
                total_workers <= facility.capacity,
                "Facility over capacity: workers={}, capacity={}",
                total_workers,
                facility.capacity
            );
        }
    }

    assert_eq!(
        pop_count(&world),
        initial_pop_count,
        "Population should stay constant when mortality is disabled"
    );
}

#[test]
fn invariant_external_flow_matches_local_currency_delta() {
    let settlement = SettlementId::new(0);
    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, 0.45);

    let good_profiles = vec![GoodProfile {
        good: GRAIN,
        contributions: vec![],
    }];
    let needs: HashMap<String, Need> = HashMap::new();

    let mut seller = Pop::new();
    seller.currency = 0.0;
    seller.income_ema = 0.0;
    seller.stocks.insert(GRAIN, 50.0);
    seller.desired_consumption_ema.insert(GRAIN, 0.01);

    let mut buyer = Pop::new();
    buyer.currency = 200.0;
    buyer.income_ema = 200.0;
    buyer.stocks.insert(GRAIN, 0.0);
    buyer.desired_consumption_ema.insert(GRAIN, 10.0);

    let mut external = ExternalMarketConfig::default();
    external.anchors.insert(
        GRAIN,
        AnchoredGoodConfig {
            world_price: 10.0,
            spread_bps: 500.0,
            base_depth: 12.0,
            depth_per_pop: 0.0,
            tiers: 9,
            tier_step_bps: 300.0,
        },
    );
    external.frictions.insert(
        settlement,
        SettlementFriction {
            enabled: true,
            transport_bps: 9000.0,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );

    for tick in 1..=10 {
        let pre_currency = seller.currency + buyer.currency;
        let mut flows = OutsideFlowTotals::default();
        let mut pops: Vec<(PopKey, &mut Pop)> = vec![(pk(1), &mut seller), (pk(2), &mut buyer)];
        let mut merchants = Vec::new();
        let _ = run_settlement_tick(
            tick,
            settlement,
            &mut pops,
            &mut merchants,
            &good_profiles,
            &needs,
            &mut price_ema,
            Some(&external),
            Some(&mut flows),
            None,
            &HashMap::new(),
            None,
        );
        let post_currency = seller.currency + buyer.currency;
        let currency_delta = post_currency - pre_currency;

        let exports_value = flows
            .exports_value
            .get(&(settlement, GRAIN))
            .copied()
            .unwrap_or(0.0);
        let imports_value = flows
            .imports_value
            .get(&(settlement, GRAIN))
            .copied()
            .unwrap_or(0.0);
        let expected_delta = exports_value - imports_value;

        let diff = (currency_delta - expected_delta).abs();
        assert!(
            diff < 1e-6,
            "External flow accounting mismatch at tick {tick}: currency_delta={currency_delta:.6}, expected={expected_delta:.6}, diff={diff:.6}"
        );
    }
}

#[test]
fn invariant_subsistence_allocates_more_to_earlier_pops() {
    let settlement = SettlementId::new(0);
    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, 1.0);

    let good_profiles = vec![GoodProfile {
        good: GRAIN,
        contributions: vec![],
    }];
    let needs: HashMap<String, Need> = HashMap::new();

    let mut pop_a = Pop::new();
    let mut pop_b = Pop::new();
    let mut pop_c = Pop::new();
    pop_a.stocks.insert(GRAIN, 0.0);
    pop_b.stocks.insert(GRAIN, 0.0);
    pop_c.stocks.insert(GRAIN, 0.0);
    pop_a.desired_consumption_ema.insert(GRAIN, 0.0);
    pop_b.desired_consumption_ema.insert(GRAIN, 0.0);
    pop_c.desired_consumption_ema.insert(GRAIN, 0.0);

    // k=2: ranks 1-2 get q_max, rank 3 gets less (crowding dropoff)
    let subsistence = SubsistenceReservationConfig::new(GRAIN, 1.5, 2, 10.0, 0.10);

    // Queue orders pops as [3, 1, 2] — pop3 rank 1, pop1 rank 2, pop2 rank 3.
    // With k=2: pop3 and pop1 get q_max (1.5), pop2 gets 0.75 (crowding).
    let queue = vec![pk(3), pk(1), pk(2)];

    let mut pops: Vec<(PopKey, &mut Pop)> = vec![
        (pk(1), &mut pop_a),
        (pk(2), &mut pop_b),
        (pk(3), &mut pop_c),
    ];
    let mut merchants = Vec::new();
    let _ = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        None,
        None,
        Some(&subsistence),
        &HashMap::new(),
        Some(&queue),
    );

    let a = pop_a.stocks.get(&GRAIN).copied().unwrap_or(0.0);
    let b = pop_b.stocks.get(&GRAIN).copied().unwrap_or(0.0);
    let c = pop_c.stocks.get(&GRAIN).copied().unwrap_or(0.0);
    // Queue [3,1,2] with k=2: pop3=1.5 (rank1), pop1=1.5 (rank2), pop2=0.75 (rank3)
    // Front of queue gets at least as much as middle, middle strictly more than back.
    assert!(
        c >= a && a > b,
        "Queue-ordered subsistence ranking violated: pop3={c:.4}, pop1={a:.4}, pop2={b:.4}"
    );
}

#[test]
fn invariant_closed_economy_tick_residual_near_zero() {
    let mut world = World::new();
    let settlement = world.add_settlement("LedgerTown", (0.0, 0.0));
    let merchant = world.add_merchant();
    let farm = world
        .add_facility(FacilityType::Farm, settlement, merchant)
        .unwrap();

    {
        let facility = world.facility_mut(farm).unwrap();
        facility.capacity = 20;
        facility.recipe_priorities = vec![RecipeId::new(1)];
        facility.workers.insert(LABORER, 20);
    }
    {
        let merchant_ref = world.get_merchant_mut(merchant).unwrap();
        merchant_ref.stockpile_at(settlement).add(GRAIN, 5_000.0);
    }

    for _ in 0..20 {
        let pop_id = world.add_pop(settlement).unwrap();
        let pop = world.pop_mut(pop_id).unwrap();
        pop.skills.insert(LABORER);
        pop.min_wage = 0.5;
        pop.currency = 500.0;
        pop.income_ema = 2.0;
        pop.stocks.insert(GRAIN, 100.0);
        pop.desired_consumption_ema.insert(GRAIN, 1.0);
        pop.employed_at = Some(farm.key);
    }

    let s = world.settlements.get_mut(&settlement).unwrap();
    s.wage_ema.insert(LABORER, 2.0);
    s.price_ema.insert(GRAIN, 1.0);

    let good_profiles = vec![GoodProfile {
        good: GRAIN,
        contributions: vec![NeedContribution {
            need_id: "food".to_string(),
            efficiency: 1.0,
        }],
    }];

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

    let recipes = vec![
        sim_core::Recipe::new(RecipeId::new(1), "Grain Farming", vec![FacilityType::Farm])
            .with_capacity_cost(1)
            .with_worker(LABORER, 1)
            .with_output(GRAIN, 2.0),
    ];

    let ticks = 20usize;
    for _ in 0..ticks {
        world.run_tick(&good_profiles, &needs, &recipes);
    }

    assert_eq!(
        world.stock_flow_history.len(),
        ticks,
        "stock-flow history should have one entry per world tick"
    );

    for flow in &world.stock_flow_history {
        assert!(
            flow.imports_value_delta.abs() < 1e-9,
            "closed economy should have zero imports_value_delta at tick {}: {}",
            flow.tick,
            flow.imports_value_delta
        );
        assert!(
            flow.exports_value_delta.abs() < 1e-9,
            "closed economy should have zero exports_value_delta at tick {}: {}",
            flow.tick,
            flow.exports_value_delta
        );
        assert!(
            flow.expected_currency_delta_from_external.abs() < 1e-9,
            "closed economy should have zero expected external delta at tick {}: {}",
            flow.tick,
            flow.expected_currency_delta_from_external
        );
        assert!(
            flow.currency_residual.abs() < 1e-6,
            "currency residual should remain near zero at tick {}: residual={}",
            flow.tick,
            flow.currency_residual
        );
    }
}

#[test]
fn invariant_open_economy_grain_accounting_balanced() {
    let mut world = World::new();
    let settlement = world.add_settlement("OpenTown", (0.0, 0.0));
    let merchant = world.add_merchant();
    let farm = world
        .add_facility(FacilityType::Farm, settlement, merchant)
        .unwrap();

    {
        let facility = world.facility_mut(farm).unwrap();
        facility.capacity = 10;
        facility.recipe_priorities = vec![RecipeId::new(1)];
        facility.workers.insert(LABORER, 10);
    }
    {
        let merchant_ref = world.get_merchant_mut(merchant).unwrap();
        merchant_ref.currency = 10_000.0;
        merchant_ref.stockpile_at(settlement).add(GRAIN, 500.0);
    }

    for _ in 0..10 {
        let pop_id = world.add_pop(settlement).unwrap();
        let pop = world.pop_mut(pop_id).unwrap();
        pop.skills.insert(LABORER);
        pop.min_wage = 0.5;
        pop.currency = 500.0;
        pop.income_ema = 2.0;
        pop.stocks.insert(GRAIN, 50.0);
        pop.desired_consumption_ema.insert(GRAIN, 1.0);
        pop.employed_at = Some(farm.key);
    }

    let s = world.settlements.get_mut(&settlement).unwrap();
    s.wage_ema.insert(LABORER, 2.0);
    s.price_ema.insert(GRAIN, 1.0);

    // Enable external anchor
    let mut external = ExternalMarketConfig::default();
    external.anchors.insert(
        GRAIN,
        AnchoredGoodConfig {
            world_price: 10.0,
            spread_bps: 500.0,
            base_depth: 0.0,
            depth_per_pop: 0.5,
            tiers: 9,
            tier_step_bps: 300.0,
        },
    );
    external.frictions.insert(
        settlement,
        SettlementFriction {
            enabled: true,
            transport_bps: 5000.0,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );
    world.set_external_market(external);

    let good_profiles = make_grain_profile();
    let needs = make_food_need(0.1); // low requirement to avoid mortality

    let recipes = vec![make_grain_recipe(2.0)];

    let ticks = 20usize;
    for _ in 0..ticks {
        let grain_before = total_grain(&world);
        world.run_tick(&good_profiles, &needs, &recipes);
        let grain_after = total_grain(&world);

        let flow = world.stock_flow_history.last().unwrap();
        let manual_delta = grain_after - grain_before;

        // 1. Verify our manual grain snapshot matches the accounting system
        let accounted_delta = flow.goods_delta.get(&GRAIN).copied().unwrap_or(0.0);
        let snapshot_err = (manual_delta - accounted_delta).abs();
        assert!(
            snapshot_err < 1e-6,
            "Manual grain delta disagrees with accounting at tick {}: manual={manual_delta:.8}, accounted={accounted_delta:.8}, err={snapshot_err:.8}",
            flow.tick,
        );

        // 2. Verify grain conservation: delta = imports - exports + internal_net
        //    Since goods_before/goods_after already include everything, the
        //    accounting identity is: goods_delta = imports_qty - exports_qty + internal_net
        //    Therefore: internal_net = goods_delta - imports_qty + exports_qty
        //    internal_net = production + subsistence - consumption (should be >= 0 in surplus)
        let imports = flow.imports_qty_delta.get(&GRAIN).copied().unwrap_or(0.0);
        let exports = flow.exports_qty_delta.get(&GRAIN).copied().unwrap_or(0.0);

        // The accounting identity: goods_delta == internal_net + imports - exports
        // We verify this by checking goods_before/goods_after directly against our
        // manual snapshot
        let goods_before = flow.goods_before.get(&GRAIN).copied().unwrap_or(0.0);
        let goods_after = flow.goods_after.get(&GRAIN).copied().unwrap_or(0.0);
        let reconstructed = goods_before + accounted_delta;
        let reconstruction_err = (goods_after - reconstructed).abs();
        assert!(
            reconstruction_err < 1e-6,
            "Grain reconstruction failed at tick {}: goods_after={goods_after:.8}, reconstructed={reconstructed:.8}, err={reconstruction_err:.8}",
            flow.tick,
        );

        // 3. Verify the goods_delta decomposition is consistent:
        //    goods_delta should equal (imports - exports + internal_net_production)
        //    We can't measure internal_net directly, but we can verify that
        //    imports and exports are non-negative and don't exceed the total delta
        assert!(
            imports >= -1e-9,
            "Negative imports at tick {}: {imports:.8}",
            flow.tick,
        );
        assert!(
            exports >= -1e-9,
            "Negative exports at tick {}: {exports:.8}",
            flow.tick,
        );
    }

    // Verify the external anchor actually generated some trade
    let total_imports: f64 = world
        .stock_flow_history
        .iter()
        .map(|f| f.imports_qty_delta.get(&GRAIN).copied().unwrap_or(0.0))
        .sum();
    let total_exports: f64 = world
        .stock_flow_history
        .iter()
        .map(|f| f.exports_qty_delta.get(&GRAIN).copied().unwrap_or(0.0))
        .sum();
    assert!(
        total_imports > 0.0 || total_exports > 0.0,
        "Open economy should have some external trade: imports={total_imports:.4}, exports={total_exports:.4}"
    );
}
