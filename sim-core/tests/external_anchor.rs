use std::collections::HashMap;

use sim_core::tick::PRICE_EMA_ALPHA;
use sim_core::{
    AnchoredGoodConfig, ExternalMarketConfig, GoodId, GoodProfile, Need, OutsideFlowTotals, Pop,
    PopId, Price, SettlementFriction, SettlementId, run_settlement_tick,
};

const GRAIN: GoodId = 1;

fn make_anchor_config(
    settlement: SettlementId,
    enabled: bool,
    base_depth: f64,
) -> ExternalMarketConfig {
    let mut config = ExternalMarketConfig::default();
    config.anchors.insert(
        GRAIN,
        AnchoredGoodConfig {
            world_price: 10.0,
            spread_bps: 500.0,
            base_depth,
            depth_per_pop: 0.0,
            tiers: 9,
            tier_step_bps: 300.0,
        },
    );
    config.frictions.insert(
        settlement,
        SettlementFriction {
            enabled,
            transport_bps: 0.0,
            tariff_bps: 0.0,
            risk_bps: 0.0,
        },
    );
    config
}

fn base_profiles_and_needs() -> (Vec<GoodProfile>, HashMap<String, Need>) {
    (
        vec![GoodProfile {
            good: GRAIN,
            contributions: vec![],
        }],
        HashMap::new(),
    )
}

#[test]
fn price_ceiling_with_import_anchor() {
    let settlement = SettlementId::new(0);
    let config = make_anchor_config(settlement, true, 30.0);
    let (good_profiles, needs) = base_profiles_and_needs();

    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, 20.0);

    let mut buyer = Pop::new(PopId::new(1), settlement);
    buyer.currency = 50_000.0;
    buyer.income_ema = 50_000.0;
    buyer.stocks.insert(GRAIN, 0.0);
    buyer.desired_consumption_ema.insert(GRAIN, 25.0);

    let mut pops: Vec<&mut Pop> = vec![&mut buyer];
    let mut merchants = Vec::new();
    let mut flows = OutsideFlowTotals::default();

    let result = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        Some(&config),
        Some(&mut flows),
        None,
    );

    let price = result
        .clearing_prices
        .get(&GRAIN)
        .copied()
        .expect("expected anchored grain trade");

    let band = 0.05;
    let step = 0.03;
    let max_import_price = 10.0 * (1.0 + band) * (1.0 + step * 8.0);
    assert!(
        price <= max_import_price + 1e-6,
        "anchored import ladder should cap local price: price={price:.4}, max={max_import_price:.4}"
    );

    let imported = flows
        .imports_qty
        .get(&(settlement, GRAIN))
        .copied()
        .unwrap_or(0.0);
    assert!(imported > 0.0, "expected positive imports under shortage");
}

#[test]
fn price_floor_with_export_anchor() {
    let settlement = SettlementId::new(0);
    let config = make_anchor_config(settlement, true, 30.0);
    let (good_profiles, needs) = base_profiles_and_needs();

    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, 10.0);

    let mut seller = Pop::new(PopId::new(1), settlement);
    seller.currency = 0.0;
    seller.income_ema = 0.0;
    seller.stocks.insert(GRAIN, 100.0);
    seller.desired_consumption_ema.insert(GRAIN, 0.1);

    let mut pops: Vec<&mut Pop> = vec![&mut seller];
    let mut merchants = Vec::new();
    let mut flows = OutsideFlowTotals::default();

    let result = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        Some(&config),
        Some(&mut flows),
        None,
    );

    let price = result
        .clearing_prices
        .get(&GRAIN)
        .copied()
        .expect("expected anchored grain trade");

    let band = 0.05;
    let step = 0.03;
    let min_export_price = 10.0 * (1.0 - band) / (1.0 + step * 8.0);
    assert!(
        price >= min_export_price - 1e-6,
        "anchored export ladder should support local floor: price={price:.4}, min={min_export_price:.4}"
    );

    let exported = flows
        .exports_qty
        .get(&(settlement, GRAIN))
        .copied()
        .unwrap_or(0.0);
    assert!(exported > 0.0, "expected positive exports under surplus");
}

#[test]
fn outside_depth_is_respected() {
    let settlement = SettlementId::new(0);
    let depth_cap = 12.0;
    let config = make_anchor_config(settlement, true, depth_cap);
    let (good_profiles, needs) = base_profiles_and_needs();

    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, 25.0);

    let mut buyer = Pop::new(PopId::new(1), settlement);
    buyer.currency = 100_000.0;
    buyer.income_ema = 100_000.0;
    buyer.stocks.insert(GRAIN, 0.0);
    buyer.desired_consumption_ema.insert(GRAIN, 100.0);

    let mut pops: Vec<&mut Pop> = vec![&mut buyer];
    let mut merchants = Vec::new();
    let mut flows = OutsideFlowTotals::default();

    let _ = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        Some(&config),
        Some(&mut flows),
        None,
    );

    let imported = flows
        .imports_qty
        .get(&(settlement, GRAIN))
        .copied()
        .unwrap_or(0.0);
    assert!(
        imported <= depth_cap + 1e-6,
        "outside imports exceeded configured cap: imports={imported:.4}, cap={depth_cap:.4}"
    );
}

#[test]
fn no_external_flow_when_disabled() {
    let settlement = SettlementId::new(0);
    let config = make_anchor_config(settlement, false, 30.0);
    let (good_profiles, needs) = base_profiles_and_needs();

    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, 20.0);

    let mut buyer = Pop::new(PopId::new(1), settlement);
    buyer.currency = 50_000.0;
    buyer.income_ema = 50_000.0;
    buyer.stocks.insert(GRAIN, 0.0);
    buyer.desired_consumption_ema.insert(GRAIN, 25.0);

    let mut pops: Vec<&mut Pop> = vec![&mut buyer];
    let mut merchants = Vec::new();
    let mut flows = OutsideFlowTotals::default();

    let _ = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        Some(&config),
        Some(&mut flows),
        None,
    );

    let imported = flows
        .imports_qty
        .get(&(settlement, GRAIN))
        .copied()
        .unwrap_or(0.0);
    let exported = flows
        .exports_qty
        .get(&(settlement, GRAIN))
        .copied()
        .unwrap_or(0.0);

    assert_eq!(imported, 0.0);
    assert_eq!(exported, 0.0);
}

#[test]
fn external_anchor_influences_price_ema_but_local_clear_dominates() {
    let settlement = SettlementId::new(0);
    let config = make_anchor_config(settlement, true, 30.0);
    let (good_profiles, needs) = base_profiles_and_needs();

    let initial_ema = 20.0;
    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, initial_ema);

    let mut buyer = Pop::new(PopId::new(1), settlement);
    buyer.currency = 50_000.0;
    buyer.income_ema = 50_000.0;
    buyer.stocks.insert(GRAIN, 0.0);
    buyer.desired_consumption_ema.insert(GRAIN, 25.0);

    let mut pops: Vec<&mut Pop> = vec![&mut buyer];
    let mut merchants = Vec::new();

    let result = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        Some(&config),
        None,
        None,
    );

    let local_price = result
        .clearing_prices
        .get(&GRAIN)
        .copied()
        .expect("expected anchored grain trade");
    let next_ema = *price_ema.get(&GRAIN).expect("grain EMA should exist");
    let world = config
        .anchors
        .get(&GRAIN)
        .expect("anchor config missing grain")
        .world_price;

    let local_only = (1.0 - PRICE_EMA_ALPHA) * initial_ema + PRICE_EMA_ALPHA * local_price;
    let world_only = (1.0 - PRICE_EMA_ALPHA) * initial_ema + PRICE_EMA_ALPHA * world;

    assert!(
        next_ema <= local_only + 1e-6,
        "external influence should pull toward world, not away: ema={next_ema:.4}, local_only={local_only:.4}"
    );
    assert!(
        next_ema >= world_only - 1e-6,
        "external influence must remain bounded and local-dominant: ema={next_ema:.4}, world_only={world_only:.4}"
    );
}

#[test]
fn anchored_no_trade_tick_still_moves_ema_toward_world() {
    let settlement = SettlementId::new(0);
    let config = make_anchor_config(settlement, true, 30.0);
    let (good_profiles, needs) = base_profiles_and_needs();

    let initial_ema = 20.0;
    let mut price_ema: HashMap<GoodId, Price> = HashMap::new();
    price_ema.insert(GRAIN, initial_ema);

    let mut idle_pop = Pop::new(PopId::new(1), settlement);
    idle_pop.currency = 0.0;
    idle_pop.income_ema = 0.0;
    idle_pop.stocks.insert(GRAIN, 0.0);
    idle_pop.desired_consumption_ema.insert(GRAIN, 0.0);

    let mut pops: Vec<&mut Pop> = vec![&mut idle_pop];
    let mut merchants = Vec::new();

    let result = run_settlement_tick(
        1,
        settlement,
        &mut pops,
        &mut merchants,
        &good_profiles,
        &needs,
        &mut price_ema,
        Some(&config),
        None,
        None,
    );

    assert!(
        !result.clearing_prices.contains_key(&GRAIN),
        "expected no local grain trade"
    );

    let next_ema = *price_ema.get(&GRAIN).expect("grain EMA should exist");
    let world = config
        .anchors
        .get(&GRAIN)
        .expect("anchor config missing grain")
        .world_price;

    assert!(
        next_ema < initial_ema,
        "anchored no-trade EMA should move toward world: next={next_ema:.4}, initial={initial_ema:.4}"
    );
    assert!(
        next_ema > world,
        "anchored no-trade EMA influence should be bounded: next={next_ema:.4}, world={world:.4}"
    );
}
