# Stability Investigation Report (Initial)

## Scope

Initial data-backed pass on stability questions using instrumented DataFrames from:

- `sim-core/tests/stability_investigation.rs`
- test: `investigate_anchor_regimes_with_dataframes` (manual/ignored)

Scenarios analyzed:

1. `balanced_candidate` (`depth_per_pop=0.10`, `transport_bps=9000`)
2. `low_price_collapse` (`depth_per_pop=0.20`, `transport_bps=11000`)
3. `high_price_spike` (`depth_per_pop=0.20`, `transport_bps=7000`)

## Method

For each scenario, run 220 ticks with instrumentation and compute:

1. Tail price and tail employment.
2. No-trade share.
3. Order-book overlap (`max pop bid - min merchant ask`).
4. Merchant fill rate (`merchant sold / merchant offered`).
5. External flow totals (imports, exports).
6. Decomposition:
   - `offered_zero_share`
   - `offered_but_no_trade_share`
   - `pop_buy_zero_share`
7. Consumption fulfillment, mortality outcomes, wage/price ratio.

## Observed Results (latest run)

`balanced_candidate`

1. `tail_p=0.4979`, `tail_emp=0.9357`
2. `no_trade=0.2773`
3. `overlap=0.4554`, `fill_rate=0.2702`
4. `imports=0.00`, `exports=15.86`
5. `offered_zero=0.0000`, `offered_no_trade=0.2773`, `pop_buy_zero=0.2773`

`low_price_collapse`

1. `tail_p=0.6230`, `tail_emp=0.9361`
2. `no_trade=0.8182`
3. `overlap=0.4516`, `fill_rate=0.0606`
4. `imports=0.00`, `exports=0.00`
5. `offered_zero=0.0000`, `offered_no_trade=0.8182`, `pop_buy_zero=0.8182`

`high_price_spike`

1. `tail_p=2.3839`, `tail_emp=0.6600`
2. `no_trade=0.0000`
3. `overlap=1.7082`, `fill_rate=0.3897`
4. `imports=0.00`, `exports=971.79`
5. `offered_zero=0.0000`, `offered_no_trade=0.0000`, `pop_buy_zero=0.0000`

## Root-Cause Analysis

## 1) No-trade in balanced/low-price regimes is not primarily book mismatch

Symptom:

1. High no-trade share (`0.2773` and `0.8182`).

Immediate mechanism:

1. `offered_zero=0` while `offered_no_trade` ~= `pop_buy_zero`.
2. Merchant is offering, but pop buy orders are often absent on no-trade ticks.

Deeper cause (upstream):

1. Pop stock/target controller frequently exits buy mode after temporary stock sufficiency.
2. This creates demand intermittency; market inactivity is demand-side controller behavior, not inability to clear when both sides post.

Why this is not superficial:

1. If mismatch were dominant, we would see `offered_no_trade` high with `pop_buy_zero` low.
2. Instead, no-trade aligns with missing buy orders.

## 2) High-price regime appears anchored by external export demand floor

Symptom:

1. Tail price materially higher (`~2.38`) with lower employment (`~0.66`).

Immediate mechanism:

1. Continuous trading (`no_trade=0`), strong overlap (`1.7082`), moderate fill rate.
2. Very large external exports (`971.79`) and zero imports.

Deeper cause (upstream):

1. Low-friction external export ladder introduces persistent high-value outside demand.
2. Outside bids support high local clearing prices and pull grain outward.
3. Local wage purchasing power is pressured (`wage/price ~ 1`), feeding demographic stress.

Why this is not superficial:

1. Price elevation co-occurs with massive export flow and no inactivity.
2. This points to active anchor-flow-driven price support, not random volatility.

## 3) Anchor engagement is regime-dependent and often dormant in weaker-anchor settings

Symptom:

1. In balanced and low-price settings, imports are ~0 and exports are tiny/zero.

Immediate mechanism:

1. External flow table shows little to no outside participation.

Deeper cause (upstream):

1. Friction/depth combinations often place outside ladders away from executed local range.
2. Local controller interactions dominate without outside correction in these regimes.

Why this matters:

1. If anchor is frequently dormant, some calibration conclusions attributed to anchor strength are actually internal-controller outcomes.

## Limitations / Confidence

1. Mortality/growth stochasticity introduces run-to-run variation.
2. This is a single-pass initial report; robust conclusions need repeated-seed aggregation.
3. Confidence is high on qualitative mechanism direction, moderate on exact magnitudes.

## Next Investigation Steps

1. Run repeated deterministic seed schedule and report `p10/p50/p90` by scenario.
2. Add tick-level causal decomposition:
   - no-trade ticks split by `no buyer` vs `book mismatch`.
3. Add scenario with intentionally import-bound shortage to measure import-side activation symmetry.
