use std::collections::HashMap;

use sim_core::{
    AnchoredGoodConfig, ExternalMarketConfig, GoodId, GoodProfile, Need, OutsideFlowTotals, Pop,
    PopId, Price, SettlementFriction, SettlementId, run_settlement_tick,
};

const GRAIN: GoodId = 1;

fn make_anchor_config(settlement: SettlementId, enabled: bool, base_depth: f64) -> ExternalMarketConfig {
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
