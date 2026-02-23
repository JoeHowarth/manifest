#[allow(dead_code)]
mod common;

use std::collections::HashMap;

use common::*;
use sim_core::{FacilityType, RecipeId, SkillId, World};

fn configure_single_facility(
    world: &mut World,
    settlement: sim_core::SettlementId,
    merchant: sim_core::MerchantId,
    bid: f64,
) {
    let facility = world
        .add_facility(FacilityType::Farm, settlement, merchant)
        .expect("facility should be created");
    {
        let f = world
            .facility_mut(facility)
            .expect("facility should exist for configuration");
        f.capacity = 1;
        f.recipe_priorities = vec![RecipeId::new(1)];
    }
    let settlement_state = world
        .settlements
        .get_mut(&settlement)
        .expect("settlement should exist");
    let bid_state = settlement_state
        .facility_bid_states
        .get_mut(facility.key)
        .expect("facility bid state should be initialized");
    bid_state.bids.insert(LABORER, bid);
}

fn add_single_laborer_pop(world: &mut World, settlement: sim_core::SettlementId) {
    let pop_handle = world
        .add_pop(settlement)
        .expect("pop should be created for settlement");
    let pop = world.pop_mut(pop_handle).expect("pop should exist");
    pop.skills.insert(LABORER);
    pop.min_wage = 0.0;
    pop.currency = 0.0;
    pop.income_ema = 0.0;
    pop.desired_consumption_ema.insert(GRAIN, 1.0);
}

fn run_one_tick(world: &mut World) {
    let good_profiles = make_grain_profile();
    let needs = make_food_need(1.0);
    let recipes = vec![make_grain_recipe(1.0)];
    world.run_tick(&good_profiles, &needs, &recipes);
}

#[test]
fn labor_output_price_uses_relevant_goods_only() {
    fn wage_paid_with_unrelated_price(unrelated_price: Option<f64>) -> f64 {
        let mut world = World::new();
        let settlement = world.add_settlement("Solo", (0.0, 0.0));
        let merchant = world.add_merchant();

        world
            .get_merchant_mut(merchant)
            .expect("merchant should exist")
            .currency = 1_000.0;

        configure_single_facility(&mut world, settlement, merchant, 500.0);
        add_single_laborer_pop(&mut world, settlement);

        let settlement_state = world
            .settlements
            .get_mut(&settlement)
            .expect("settlement should exist");
        settlement_state.wage_ema.insert(LABORER, 1.0);
        if let Some(price) = unrelated_price {
            settlement_state.price_ema.insert(999, price);
        }

        run_one_tick(&mut world);

        let settlement_state = world
            .settlements
            .get(&settlement)
            .expect("settlement should exist after tick");
        let pop = settlement_state
            .pops
            .values()
            .next()
            .expect("one pop should exist after tick");
        pop.currency
    }

    let wage_without_unrelated = wage_paid_with_unrelated_price(None);
    let wage_with_unrelated = wage_paid_with_unrelated_price(Some(10_000.0));

    assert!(
        (wage_without_unrelated - wage_with_unrelated).abs() < 1e-9,
        "Unrelated price_ema entry changed labor wage: without={wage_without_unrelated}, with={wage_with_unrelated}"
    );
    assert!(
        wage_with_unrelated < 5.0,
        "Relevant-output pricing should keep wage near recipe output value, got {wage_with_unrelated}"
    );
}

#[test]
fn labor_bootstraps_without_seeded_wage_ema() {
    let mut world = World::new();
    let settlement = world.add_settlement("Bootstrap", (0.0, 0.0));
    let merchant = world.add_merchant();
    world
        .get_merchant_mut(merchant)
        .expect("merchant should exist")
        .currency = 100.0;

    configure_single_facility(&mut world, settlement, merchant, 2.0);
    add_single_laborer_pop(&mut world, settlement);

    // Intentionally do not seed settlement.wage_ema.
    run_one_tick(&mut world);

    let settlement_state = world
        .settlements
        .get(&settlement)
        .expect("settlement should exist after tick");
    let pop = settlement_state
        .pops
        .values()
        .next()
        .expect("one pop should exist after tick");

    assert!(
        pop.employed_at.is_some(),
        "Pop should be hired even with initially empty wage_ema"
    );
    assert!(
        settlement_state.wage_ema.contains_key(&LABORER),
        "Labor phase should bootstrap missing wage_ema entries from runtime skills"
    );
}

#[test]
fn no_unpaid_employed_pops() {
    let mut world = World::new();
    let settlement = world.add_settlement("BudgetTown", (0.0, 0.0));
    let merchant = world.add_merchant();
    world
        .get_merchant_mut(merchant)
        .expect("merchant should exist")
        .currency = 1.5;

    configure_single_facility(&mut world, settlement, merchant, 1.0);
    configure_single_facility(&mut world, settlement, merchant, 1.0);
    add_single_laborer_pop(&mut world, settlement);
    add_single_laborer_pop(&mut world, settlement);

    let settlement_state = world
        .settlements
        .get_mut(&settlement)
        .expect("settlement should exist");
    settlement_state.wage_ema.insert(LABORER, 1.0);

    run_one_tick(&mut world);

    let settlement_state = world
        .settlements
        .get(&settlement)
        .expect("settlement should exist after tick");
    let employed_count = settlement_state
        .pops
        .values()
        .filter(|p| p.employed_at.is_some())
        .count();
    let paid_count = settlement_state
        .pops
        .values()
        .filter(|p| p.currency > 0.0)
        .count();

    assert_eq!(
        employed_count, paid_count,
        "No pop should end employed without actually receiving wages"
    );

    let merchant_currency = world
        .get_merchant(merchant)
        .expect("merchant should still exist")
        .currency;
    assert!(
        merchant_currency >= -1e-9,
        "Merchant currency should never go negative, got {merchant_currency}"
    );
}

#[test]
fn settlement_order_invariance_for_shared_merchant() {
    fn run_two_settlement_case(first: &str, second: &str) -> HashMap<String, usize> {
        let mut world = World::new();
        let first_id = world.add_settlement(first, (0.0, 0.0));
        let second_id = world.add_settlement(second, (1.0, 0.0));
        let merchant = world.add_merchant();
        world
            .get_merchant_mut(merchant)
            .expect("merchant should exist")
            .currency = 1.5;

        configure_single_facility(&mut world, first_id, merchant, 1.0);
        configure_single_facility(&mut world, second_id, merchant, 1.0);
        add_single_laborer_pop(&mut world, first_id);
        add_single_laborer_pop(&mut world, second_id);

        world
            .settlements
            .get_mut(&first_id)
            .expect("first settlement should exist")
            .wage_ema
            .insert(SkillId(1), 1.0);
        world
            .settlements
            .get_mut(&second_id)
            .expect("second settlement should exist")
            .wage_ema
            .insert(SkillId(1), 1.0);

        run_one_tick(&mut world);

        world
            .settlements
            .values()
            .map(|s| {
                let employed = s.pops.values().filter(|p| p.employed_at.is_some()).count();
                (s.info.name.clone(), employed)
            })
            .collect()
    }

    let forward = run_two_settlement_case("Alpha", "Beta");
    let reversed = run_two_settlement_case("Beta", "Alpha");

    assert_eq!(
        forward.get("Alpha"),
        reversed.get("Alpha"),
        "Alpha outcomes should not depend on settlement creation order"
    );
    assert_eq!(
        forward.get("Beta"),
        reversed.get("Beta"),
        "Beta outcomes should not depend on settlement creation order"
    );
}

#[test]
fn settlement_order_invariance_for_shared_merchant_multi_tick() {
    #[derive(Debug, Clone, PartialEq)]
    struct Snapshot {
        employed_by_name: HashMap<String, usize>,
        pop_count_by_name: HashMap<String, usize>,
        merchant_currency: f64,
        total_employed: usize,
    }

    fn scenario(first: &str, second: &str) -> Snapshot {
        let mut world = World::with_seed(7);
        world.mortality_grace_ticks = 1_000;

        let first_id = world.add_settlement(first, (0.0, 0.0));
        let second_id = world.add_settlement(second, (1.0, 0.0));
        let merchant = world.add_merchant();
        world
            .get_merchant_mut(merchant)
            .expect("merchant should exist")
            .currency = 4.0;

        configure_single_facility(&mut world, first_id, merchant, 1.0);
        configure_single_facility(&mut world, second_id, merchant, 1.0);
        for _ in 0..3 {
            add_single_laborer_pop(&mut world, first_id);
            add_single_laborer_pop(&mut world, second_id);
        }

        world
            .settlements
            .get_mut(&first_id)
            .expect("first settlement should exist")
            .wage_ema
            .insert(LABORER, 1.0);
        world
            .settlements
            .get_mut(&second_id)
            .expect("second settlement should exist")
            .wage_ema
            .insert(LABORER, 1.0);

        let empty_profiles = Vec::new();
        let empty_needs = HashMap::new();
        let recipes = vec![make_grain_recipe(1.0)];
        for _ in 0..8 {
            world.run_tick(&empty_profiles, &empty_needs, &recipes);
        }

        let employed_by_name: HashMap<String, usize> = world
            .settlements
            .values()
            .map(|s| {
                (
                    s.info.name.clone(),
                    s.pops.values().filter(|p| p.employed_at.is_some()).count(),
                )
            })
            .collect();
        let pop_count_by_name: HashMap<String, usize> = world
            .settlements
            .values()
            .map(|s| (s.info.name.clone(), s.pops.len()))
            .collect();

        Snapshot {
            total_employed: employed_by_name.values().sum(),
            employed_by_name,
            pop_count_by_name,
            merchant_currency: world
                .get_merchant(merchant)
                .expect("merchant should exist after ticks")
                .currency,
        }
    }

    let forward = scenario("Alpha", "Beta");
    let reversed = scenario("Beta", "Alpha");

    assert_eq!(
        forward.employed_by_name.get("Alpha"),
        reversed.employed_by_name.get("Alpha"),
        "Alpha employment should not depend on settlement creation order"
    );
    assert_eq!(
        forward.employed_by_name.get("Beta"),
        reversed.employed_by_name.get("Beta"),
        "Beta employment should not depend on settlement creation order"
    );
    assert_eq!(
        forward.pop_count_by_name.get("Alpha"),
        reversed.pop_count_by_name.get("Alpha"),
        "Alpha population count should match across orderings"
    );
    assert_eq!(
        forward.pop_count_by_name.get("Beta"),
        reversed.pop_count_by_name.get("Beta"),
        "Beta population count should match across orderings"
    );
    assert_eq!(
        forward.total_employed, reversed.total_employed,
        "Total employment should be invariant across orderings"
    );
    assert!(
        (forward.merchant_currency - reversed.merchant_currency).abs() < 1e-9,
        "Merchant currency should match across orderings: forward={}, reversed={}",
        forward.merchant_currency,
        reversed.merchant_currency
    );
}

#[test]
fn duplicate_settlement_names_do_not_introduce_labor_nondeterminism() {
    fn scenario(order_forward: bool) -> (usize, usize) {
        let mut world = World::with_seed(7);
        world.mortality_grace_ticks = 1_000;

        let (first_id, second_id) = if order_forward {
            (
                world.add_settlement("Same", (0.0, 0.0)),
                world.add_settlement("Same", (1.0, 0.0)),
            )
        } else {
            (
                world.add_settlement("Same", (1.0, 0.0)),
                world.add_settlement("Same", (0.0, 0.0)),
            )
        };
        let merchant = world.add_merchant();
        world
            .get_merchant_mut(merchant)
            .expect("merchant should exist")
            .currency = 1.0;

        configure_single_facility(&mut world, first_id, merchant, 1.0);
        configure_single_facility(&mut world, second_id, merchant, 1.0);
        add_single_laborer_pop(&mut world, first_id);
        add_single_laborer_pop(&mut world, second_id);

        world
            .settlements
            .get_mut(&first_id)
            .expect("first settlement should exist")
            .wage_ema
            .insert(LABORER, 1.0);
        world
            .settlements
            .get_mut(&second_id)
            .expect("second settlement should exist")
            .wage_ema
            .insert(LABORER, 1.0);

        let empty_profiles = Vec::new();
        let empty_needs = HashMap::new();
        let recipes = vec![make_grain_recipe(1.0)];
        world.run_tick(&empty_profiles, &empty_needs, &recipes);

        let first_employed = world
            .pops_at(first_id)
            .filter(|(_, pop)| pop.employed_at.is_some())
            .count();
        let second_employed = world
            .pops_at(second_id)
            .filter(|(_, pop)| pop.employed_at.is_some())
            .count();
        (first_employed, second_employed)
    }

    let baseline_forward = scenario(true);
    let baseline_reversed = scenario(false);
    for _ in 0..20 {
        assert_eq!(
            scenario(true),
            baseline_forward,
            "forward duplicate-name run produced nondeterministic outcome"
        );
        assert_eq!(
            scenario(false),
            baseline_reversed,
            "reversed duplicate-name run produced nondeterministic outcome"
        );
    }
}
