# Aspirational Direction Spec (Builds on Current Runtime)

## Purpose

State the target architecture and migration path from current implemented behavior.
This doc is directional by design. For current behavior, see:

1. `TICK_STATE_SPEC.md`
2. `MARKETS_LABOR_SPEC.md`
3. `CONVERGENCE_INVARIANTS_SPEC.md`

## Design North Star

The medium-term target is a stock-flow-consistent economy with:

1. Survival-first pop dynamics that avoid collapse artifacts.
2. Labor pricing grounded in subsistence opportunity cost.
3. Goods prices stabilized by bounded external anchoring, not hard pegs.
4. Convergence assessed on explicit equilibrium conditions, not only heuristics.

## What We Already Have (Foundation)

Current runtime already provides strong primitives:

1. Modular tick phases with clear extension points.
2. Budget- and inventory-constrained call auctions.
3. Adaptive facility wage bidding.
4. In-kind subsistence and subsistence-derived reservation ladder.
5. Optional external import/export ladder with finite depth.
6. Convergence and invariant test harness with scenario sweeps.

These are sufficient to evolve incrementally without rewriting the engine.

## Target Model by Subsystem

### 1) Population and Demography

Target:

1. Demography responds to rolling intake and shortfall duration, not only one-tick shock.
2. Surplus intake supports bounded growth.
3. Starvation risk is tied to sustained deficits, reducing brittle extinction cascades.

Build path:

1. Keep existing `need_satisfaction` channel.
2. Add rolling food-intake state per pop.
3. Move death/growth probabilities to consume rolling signals.
4. Recalibrate convergence thresholds to new demographic response times.

### 2) Labor Market and Reservation

Target:

1. Reservation wage derived from outside option productivity (subsistence), not static floor.
2. Optional transition to demand-side-dominant labor clearing if desired.
3. Wage formation should remain stable under supply shocks.

Build path:

1. Keep current ask-based market.
2. Strengthen subsistence reservation ladder semantics as primary reservation source.
3. Optionally introduce askless mode behind feature/config gate and A/B compare in tests.
4. Promote whichever mode yields better convergence and fewer pathological regimes.

### 3) Goods Market and Price Stability

Target:

1. Grain-centric soft numeraire with bounded arbitrage band.
2. External anchor participation restricted to designated port settlements.
3. Non-port settlements receive anchor influence indirectly through merchant-owned ships/caravans.
4. Other goods float, interpreted in grain-relative terms.
5. Price stabilization from finite external liquidity plus endogenous local trade.

Build path:

1. Retain current outside ladder architecture.
2. Start with grain-only anchor.
3. Add settlement role flag (`port` vs `non-port`) and gate outside ladders to ports only.
4. Add merchant transport-route dependence so non-ports only access anchor via internal trade flows.
5. Tune depth/friction to avoid over-dominating local market.
6. Add explicit diagnostics for inside-band neutrality vs persistent import/export dependence.
7. Add diagnostics for inland port-dependence, route bottlenecks, and regional price dispersion.

### 4) Stock-Flow Accounting

Target:

1. Economic imbalances are diagnosable from explicit stock-flow equations.
2. Currency and goods conservation assumptions are explicit by regime.
3. External account exposure is first-class and auditable.

Build path:

1. Keep existing invariant tests.
2. Add per-tick stock-flow decomposition outputs to instrumentation.
3. Add regression tests on decomposition residuals.
4. Require residuals near zero in closed-economy scenarios.

### 5) Convergence Framework

Target:

1. Scenario suite covers both balanced and highly imbalanced starts.
2. Passing implies survival and bounded oscillation under realistic shocks.
3. Stress scenarios become progressively promoted from characterization to gating as stability improves.

Build path:

1. Keep current strict/weak split.
2. Add analytic equilibrium references per scenario family.
3. Add parameter sweeps over key control knobs.
4. Tighten gating only after repeated stable runs.

## Staged Implementation Plan

### Stage A: Observability First

1. Add explicit stock-flow and outside-account diagnostics.
2. Persist run outputs as parquet for sweep analysis.
3. Add simple dashboards/queries for imbalance root-cause detection.

### Stage B: Demography and Reservation Hardening

1. Add rolling-intake demography state.
2. Integrate subsistence reservation as default reservation mechanism.
3. Retune mortality/growth and labor bid controls jointly.

### Stage C: Anchor and Market Robustness

1. Settle grain-only external anchor policy.
2. Implement and test port-only external access topology.
3. Implement merchant transport mediation for inland access.
4. Sweep depth/friction and route-capacity parameters to find stable operating envelope.
5. Gate against over-reliance on outside imports in nominally self-sustaining scenarios.

### Stage D: Promotion of Stronger Gates

1. Promote selected stress scenarios to gating.
2. Tighten strict convergence thresholds in increments.
3. Lock in target baselines with deterministic seeds where possible.

## Decision Principles

When tradeoffs appear, prioritize:

1. Stock-flow coherence over convenience heuristics.
2. Stable feedback loops over short-term metric improvements.
3. Incremental, test-backed migration over large rewrites.
4. Explicit regime flags over hidden behavior changes.

## Exit Criteria for “Convergence-Ready” Core

The core is considered convergence-ready when:

1. Balanced and imbalanced gated scenarios achieve high survival and stable tails.
2. No known invariant violations under enabled stabilizers.
3. External anchor usage is mostly corrective, not permanently load-bearing.
4. Population settles near analytically plausible operating ranges across sweeps.
