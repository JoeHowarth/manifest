use std::collections::HashMap;

use sim_core::{
    labor::SkillId,
    production::{FacilityType, RecipeId},
    run_settlement_tick, AnchoredGoodConfig, ExternalMarketConfig, GoodId, GoodProfile, Need,
    NeedContribution, OutsideFlowTotals, Pop, PopId, Price, Recipe, SettlementFriction,
    SettlementId, SubsistenceReservationConfig, UtilityCurve, World,
};

const GRAIN: GoodId = 1;
const LABORER: SkillId = SkillId(1);

fn total_currency(world: &World) -> f64 {
    let pop_currency: f64 = world.pops.values().map(|p| p.currency).sum();
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
        let pop = world.get_pop_mut(pop_id).unwrap();
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
    world.run_tick(&good_profiles, &needs, &Vec::<Recipe>::new());
    let final_currency = total_currency(&world);

    let diff = (final_currency - initial_currency).abs();
    assert!(
        diff < 1e-6,
        "Currency should be conserved even with growth: initial={initial_currency:.2}, final={final_currency:.2}, diff={diff:.2}"
    );
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

    let mut seller = Pop::new(PopId::new(1), settlement);
    seller.currency = 0.0;
    seller.stocks.insert(GRAIN, 2.0); // tiny inventory
    seller.desired_consumption_ema.insert(GRAIN, 0.001); // target=0.005 => near-full stock is "excess"

    let mut buyer = Pop::new(PopId::new(2), settlement);
    buyer.currency = 10_000.0;
    buyer.income_ema = 10_000.0;
    buyer.stocks.insert(GRAIN, 0.0);
    buyer.desired_consumption_ema.insert(GRAIN, 10.0); // very strong demand

    let initial_stock = seller.stocks.get(&GRAIN).copied().unwrap_or(0.0);

    let mut pops: Vec<&mut Pop> = vec![&mut seller, &mut buyer];
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
        let facility = world.get_facility_mut(farm_a).unwrap();
        facility.capacity = 50;
        facility.recipe_priorities = vec![RecipeId::new(1)];
        facility.workers.insert(LABORER, 50);
    }
    {
        let facility = world.get_facility_mut(farm_b).unwrap();
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
        let pop = world.get_pop_mut(pop_id).unwrap();
        pop.skills.insert(LABORER);
        pop.min_wage = 0.5;
        pop.currency = 100.0;
        pop.income_ema = 1.0;
        pop.stocks.insert(GRAIN, 5.0);
        pop.desired_consumption_ema.insert(GRAIN, 1.0);
        pop.employed_at = Some(if i % 2 == 0 { farm_a } else { farm_b });
    }

    world.wage_ema.insert(LABORER, 1.0);
    world.price_ema.insert((settlement, GRAIN), 1.0);

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

    let recipes =
        vec![
            sim_core::Recipe::new(RecipeId::new(1), "Grain Farming", vec![FacilityType::Farm])
                .with_capacity_cost(1)
                .with_worker(LABORER, 1)
                .with_output(GRAIN, 1.0),
        ];

    let initial_pop_count = world.pops.len();
    for _ in 0..80 {
        world.run_tick(&good_profiles, &needs, &recipes);

        let employed_now = world
            .pops
            .values()
            .filter(|p| p.employed_at.is_some())
            .count();
        let workers_in_facilities: usize = world
            .facilities
            .values()
            .map(|f| f.workers.values().sum::<u32>() as usize)
            .sum();

        assert_eq!(
            employed_now, workers_in_facilities,
            "Employment/facility worker mismatch: employed={employed_now}, facility_workers={workers_in_facilities}"
        );
        assert!(
            employed_now <= world.pops.len(),
            "More employed pops than total pops: employed={employed_now}, total={}",
            world.pops.len()
        );

        for facility in world.facilities.values() {
            let total_workers: u32 = facility.workers.values().sum();
            assert!(
                total_workers <= facility.capacity,
                "Facility over capacity: facility={:?}, workers={}, capacity={}",
                facility.id,
                total_workers,
                facility.capacity
            );
        }
    }

    assert_eq!(
        world.pops.len(),
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

    let mut seller = Pop::new(PopId::new(1), settlement);
    seller.currency = 0.0;
    seller.income_ema = 0.0;
    seller.stocks.insert(GRAIN, 50.0);
    seller.desired_consumption_ema.insert(GRAIN, 0.01);

    let mut buyer = Pop::new(PopId::new(2), settlement);
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

    let initial_currency = seller.currency + buyer.currency;
    let mut flows = OutsideFlowTotals::default();
    let mut pops: Vec<&mut Pop> = vec![&mut seller, &mut buyer];
    let mut merchants = Vec::new();
    let _ = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        Some(&external),
        Some(&mut flows),
        None,
    );
    let final_currency = seller.currency + buyer.currency;
    let currency_delta = final_currency - initial_currency;

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
        "External flow accounting mismatch: currency_delta={currency_delta:.6}, expected={expected_delta:.6}, diff={diff:.6}"
    );
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

    let mut pop_a = Pop::new(PopId::new(1), settlement);
    let mut pop_b = Pop::new(PopId::new(2), settlement);
    let mut pop_c = Pop::new(PopId::new(3), settlement);
    pop_a.stocks.insert(GRAIN, 0.0);
    pop_b.stocks.insert(GRAIN, 0.0);
    pop_c.stocks.insert(GRAIN, 0.0);
    pop_a.desired_consumption_ema.insert(GRAIN, 0.0);
    pop_b.desired_consumption_ema.insert(GRAIN, 0.0);
    pop_c.desired_consumption_ema.insert(GRAIN, 0.0);

    let subsistence = SubsistenceReservationConfig {
        grain_good: GRAIN,
        q_max: 3.0,
        crowding_alpha: 1.0,
        default_grain_price: 10.0,
    };

    let mut pops: Vec<&mut Pop> = vec![&mut pop_a, &mut pop_b, &mut pop_c];
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
    );

    let a = pop_a.stocks.get(&GRAIN).copied().unwrap_or(0.0);
    let b = pop_b.stocks.get(&GRAIN).copied().unwrap_or(0.0);
    let c = pop_c.stocks.get(&GRAIN).copied().unwrap_or(0.0);
    assert!(
        a > b && b > c,
        "Subsistence ranking violated: pop1={a:.4}, pop2={b:.4}, pop3={c:.4}"
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
        let facility = world.get_facility_mut(farm).unwrap();
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
        let pop = world.get_pop_mut(pop_id).unwrap();
        pop.skills.insert(LABORER);
        pop.min_wage = 0.5;
        pop.currency = 500.0;
        pop.income_ema = 2.0;
        pop.stocks.insert(GRAIN, 100.0);
        pop.desired_consumption_ema.insert(GRAIN, 1.0);
        pop.employed_at = Some(farm);
    }

    world.wage_ema.insert(LABORER, 2.0);
    world.price_ema.insert((settlement, GRAIN), 1.0);

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
                // Keep sat pinned near 1.0 so mortality/growth RNG does not fire.
                requirement: 0.1,
                steepness: 5.0,
            },
        },
    );

    let recipes =
        vec![
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
