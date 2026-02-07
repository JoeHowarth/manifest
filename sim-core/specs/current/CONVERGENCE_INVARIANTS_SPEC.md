# Convergence and Invariants Spec (Current Runtime)

## Purpose

Describe what convergence means in current tests, what is gated, and which invariants are actively enforced.

Primary references:

- `sim-core/tests/convergence.rs`
- `sim-core/tests/invariants.rs`
- `sim-core/tests/properties.rs`
- `sim-core/tests/external_anchor.rs`

## Convergence Test Structure

Convergence logic is centered in `sim-core/tests/convergence.rs`.

## Trial model

`run_multi_pop_trial_with_controls(...)` runs a multi-pop scenario and records:

1. Price history.
2. Pop count history.
3. Employment history.
4. Merchant stock history.
5. Net external flow history.
6. Food satisfaction history.

## Analytical pre-check

`predict_equilibrium_population(...)` computes an approximate feasible population range from:

1. Formal production capacity.
2. Consumption requirement.
3. Optional subsistence output function.

This is used as a sanity reference before asserting scenario outcomes.

## Strict convergence criteria

`evaluate_convergence(...)` defines strict convergence as all of:

1. No extinction.
2. Price std in trailing window <= max threshold.
3. Absolute pop slope <= max threshold.
4. Absolute merchant stock slope <= max threshold.
5. Mean employment rate >= min threshold.
6. Mean food satisfaction >= min threshold.

Default thresholds are set in `ConvergenceThresholds::default()`.

## Weak stability criteria

`is_weakly_stable(...)` defines weaker conditions:

1. No extinction.
2. Price std <= weak threshold.
3. Employment rate >= weak minimum.
4. Food satisfaction mean >= weak minimum.

Weak stability is used as the success criterion in key sweep gating.

## Gated vs non-gated scenarios

In `multi_pop_sweep_initial_conditions()`:

1. Moderate scenarios are gating with minimum success-rate requirements across reps.
2. Stress scenarios are characterization-only (printed metrics, non-gating).

This intentionally separates regressions from exploratory diagnostics.

## Instrumentation and DataFrames

The workspace includes an instrumentation crate (`instrument`) that can:

1. Capture tracing events as typed tables.
2. Convert tables to polars DataFrames.
3. Persist DataFrames as parquet.

Convergence test file includes analysis helpers over DataFrames, but these are currently helper functions rather than always-on gating checks.

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

