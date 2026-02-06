use std::collections::HashMap;

use sim_core::{
    GoodId, GoodProfile, Need, NeedContribution, Pop, PopId, Price, Recipe, SettlementId,
    UtilityCurve, World, generate_demand_curve_orders, labor::SkillId,
    market::{PriceBias, Side, clear_multi_market},
    production::{FacilityType, RecipeId},
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
    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, 1.0);

    let good_profiles = vec![GoodProfile {
        good: GRAIN,
        contributions: vec![],
    }];

    let mut seller = Pop::new(PopId::new(1), SettlementId::new(0));
    seller.currency = 0.0;
    seller.stocks.insert(GRAIN, 2.0); // tiny inventory
    seller.desired_consumption_ema.insert(GRAIN, 0.001); // target=0.005 => near-full stock is "excess"

    let mut buyer = Pop::new(PopId::new(2), SettlementId::new(0));
    buyer.currency = 10_000.0;
    buyer.income_ema = 10_000.0;
    buyer.stocks.insert(GRAIN, 0.0);
    buyer.desired_consumption_ema.insert(GRAIN, 10.0); // very strong demand

    let mut orders = Vec::new();
    orders.extend(generate_demand_curve_orders(
        &seller,
        &good_profiles,
        &price_ema,
    ));
    orders.extend(generate_demand_curve_orders(
        &buyer,
        &good_profiles,
        &price_ema,
    ));
    for (id, order) in orders.iter_mut().enumerate() {
        order.id = id as u64;
    }

    let budgets = HashMap::from([(seller.id.0, seller.currency), (buyer.id.0, buyer.currency)]);
    let initial_stock = seller.stocks.get(&GRAIN).copied().unwrap_or(0.0);

    let result = clear_multi_market(&[GRAIN], orders, &budgets, None, 20, PriceBias::FavorSellers);

    let sold: f64 = result
        .fills
        .iter()
        .filter(|f| f.agent_id == seller.id.0 && matches!(f.side, Side::Sell))
        .map(|f| f.quantity)
        .sum();
    let remaining = initial_stock - sold;
    assert!(
        remaining >= -1e-6,
        "Pop seller oversold inventory: initial={initial_stock:.4}, sold={sold:.4}, remaining={remaining:.4}"
    );
}

#[test]
fn invariant_balanced_economy_keeps_nonzero_employment() {
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

    let recipes = vec![
        sim_core::Recipe::new(RecipeId::new(1), "Grain Farming", vec![FacilityType::Farm])
            .with_capacity_cost(1)
            .with_worker(LABORER, 1)
            .with_output(GRAIN, 1.0),
    ];

    let initial_pop_count = world.pops.len();
    let mut saw_zero_employment = false;
    for _ in 0..80 {
        world.run_tick(&good_profiles, &needs, &recipes);

        let employed_now = world.pops.values().filter(|p| p.employed_at.is_some()).count();
        if employed_now == 0 {
            saw_zero_employment = true;
            break;
        }
    }

    let employed = world.pops.values().filter(|p| p.employed_at.is_some()).count();

    assert_eq!(
        world.pops.len(),
        initial_pop_count,
        "Population should stay constant when mortality is disabled"
    );
    assert!(
        !saw_zero_employment && employed > 0,
        "Balanced economy should not drop to zero employment in steady state; employed={employed}, saw_zero={saw_zero_employment}"
    );
}
