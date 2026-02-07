# Stability Investigation Task

## Purpose

Determine whether current economic dynamics are robust or only conditionally stable, and identify root causes for remaining instability/sensitivity.

## Core Questions

1. Is observed stability robust across repeated runs at fixed parameters, or seed/order sensitive?
2. Are external anchors materially participating in price formation, or mostly dormant in key scenarios?
3. Which parameter regions are robust (price/employment/import reliance in-band), and where do regimes fail?
4. For each failure regime, what is the first-order symptom and deeper causal chain?
5. Are we preserving intended model semantics:
   - soft bounded anchor (not hard peg),
   - local market primary signal,
   - non-neutral labor market behavior?

## Metrics (Primary)

1. Tail price level and dispersion (`p50`, `p10`, `p90`, std).
2. Tail employment rate (`p50`, `p10`).
3. Tail import reliance:
   - per tick `imports_qty / pop_count`,
   - cumulative share of consumption satisfied by imports.
4. Tail food satisfaction (`mean`, `min`) and pop slope.
5. Market microstructure indicators:
   - order-book overlap (`max bid` vs `min ask`),
   - filled volume / offered volume,
   - no-trade tick share.

## Experimental Design

### Scenarios

Use at least three anchor regimes:

1. `balanced`: expected pass region.
2. `low-price`: high-friction / weak-anchor regime where prices collapse.
3. `high-price`: low-friction / strong-anchor regime where prices spike.

### Replication

1. Multiple reps per scenario.
2. Deterministic seed schedule for reproducibility.
3. Fixed tick horizon and tail window for all comparisons.

### Data Capture

1. Run with instrumentation enabled.
2. Persist table parquet outputs per scenario/run.
3. Query with polars/duckdb for aggregate and causal diagnostics.

## Root-Cause Protocol (Non-Superficial)

For every conclusion:

1. State the symptom.
2. Identify immediate mechanism from data.
3. Ask: "what upstream controller/state produced that mechanism?"
4. Validate with at least one counterfactual comparison:
   - same setup with one parameter changed,
   - or same parameter with different regime evidence.
5. Reject conclusions that rely only on one aggregate metric without micro evidence.

Example standard:

- Superficial: "price is low because anchor is weak."
- Required depth: "price is low because no-trade share rises, pop bids are budget-capped below merchant asks, and EMA drifts down due low executed-price support; weak anchor only fails to counteract this because outside ladder fills are near-zero."

## Outputs

1. Investigation report with:
   - findings by question,
   - root-cause trees per failure regime,
   - confidence and evidence quality.
2. Candidate fixes ranked by expected impact and risk.
3. Follow-up validation plan tied to measurable acceptance criteria.

## Acceptance Criteria

Investigation phase is complete when:

1. Each core question has evidence-backed answer.
2. At least one deeper causal chain is documented for each observed failure regime.
3. Proposed next change is justified by data and falsifiable in tests.
