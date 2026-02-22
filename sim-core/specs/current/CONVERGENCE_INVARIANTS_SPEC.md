# Convergence and Invariants Spec (Current Runtime)

## Purpose

Describe what convergence means in current tests, what is gated, and which invariants are actively enforced.

Primary references:

- `sim-core/tests/single_good.rs`
- `sim-core/tests/carrying_capacity.rs`
- `sim-core/tests/invariants.rs`
- `sim-core/tests/properties.rs`
- `sim-core/tests/external_anchor.rs`

## Convergence Test Structure

Convergence checks are currently implemented in:

1. `sim-core/tests/single_good.rs` (multi-scenario convergence characterization around a single food good).
2. `sim-core/tests/carrying_capacity.rs` (subsistence-only carrying-capacity sweep).

## Trial model

`run_scenario_named(...)` in `single_good.rs` runs a scenario and records:

1. Price history.
2. Pop count history.
3. Employment history.
4. Food satisfaction history.

## Analytical pre-check

`predict_equilibrium_pop(...)` computes an approximate feasible population range from:

1. Formal production capacity.
2. Consumption requirement.
3. Optional subsistence output function.

This is used as a sanity reference before asserting scenario outcomes.

## Current convergence assertions

In `single_good.rs`, active default-suite tests assert combinations of:

1. No extinction.
2. Population tail mean near analytical prediction bands.
3. Employment and food-satisfaction minima.
4. Price attractor bounds in the standard scenario.
5. Collapse behavior when subsistence capacity is intentionally reduced.

`single_good.rs` also contains broader sweep characterization tests
(`varying_starting_conditions`, `varying_k_shifts_equilibrium`) that are
currently `#[ignore]` and intended for explicit runs.

In `carrying_capacity.rs`, active sweeps assert:

1. Tail stability across repeated reps per start condition.
2. Tight carrying-capacity band across wide initial populations.
3. Agreement between simulated carrying capacity and analytical subsistence prediction.

## Instrumentation and DataFrames

The workspace includes an instrumentation crate (`instrument`) that can:

1. Capture tracing events as typed tables.
2. Convert tables to polars DataFrames.
3. Persist DataFrames as parquet.

`single_good.rs` uses this instrumentation path for DataFrame-backed tail metrics and is compiled only when the `instrument` feature is enabled.

## Invariants (Targeted)

From `sim-core/tests/invariants.rs`:

1. Currency conservation under population growth.
2. Pop cannot sell more inventory than owned.
3. Labor assignment accounting consistency.
4. External flow accounting matches local currency delta.
5. Subsistence ranking allocation monotonicity.

## Property checks (broad safety)

From `sim-core/tests/properties.rs`:

1. No negative stocks.
2. No negative currency.
3. Tick counter monotonicity.
4. Price EMA bounded and finite.
5. Referential consistency of settlement-pop links.
6. Multi-settlement isolation sanity checks.

## External anchor checks

From `sim-core/tests/external_anchor.rs`:

1. Import ladder caps shortage-side price spikes.
2. Export ladder supports surplus-side price floor.
3. Outside depth caps are respected.
4. Disabled config yields no external flows.

## Current Interpretation Guidance

Use this hierarchy when evaluating system health:

1. Invariants and property tests must hold.
2. Gated convergence scenarios should meet success-rate thresholds.
3. Stress scenario outputs indicate robustness envelope, not strict correctness failure by themselves.
