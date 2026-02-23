# SoA Runtime Fix Plan

## Scope

This plan addresses four confirmed runtime issues in `sim-core`:

1. Merchant labor budget double-counting across facilities, which can produce unpaid-but-employed pops.
2. Labor phase deadlock when `wage_ema` is not pre-seeded.
3. Settlement-order-dependent outcomes caused by per-settlement full-phase ticking.
4. Arbitrary labor MVP pricing from `price_ema.values().next()` (hash iteration artifact).

## Goals

1. Remove correctness bugs and nondeterministic behavior.
2. Preserve existing desired behavior where possible.
3. Add regression tests so these issues cannot silently reappear.

## Execution Order

1. Add failing regression tests for all four issues.
2. Fix problem 4 (MVP pricing source).
3. Fix problem 2 (wage EMA bootstrap).
4. Fix problem 1 (owner-level budget/payment consistency).
5. Fix problem 3 (phase orchestration / settlement-order coupling).
6. Run full validation and clean up obsolete code/comments.

## Problem 1: Merchant Budget Double-Counting

### Problem

Each facility is cleared with the owner's full merchant currency as if independent, then wages are paid against one shared merchant balance. This can create:

1. Pops with `employed_at = Some(...)` but no wage paid.
2. Distorted income EMA and subsistence eligibility.
3. Corrupted adaptive wage feedback.

### Proposed Fix

1. Enforce labor affordability at owner level, not per facility clone.
2. Reserve wage spend during matching across all facilities owned by that merchant.
3. Commit assignment only if payable.
4. Never leave a pop employed when wage payment failed.

### Exit Criteria

1. No employed pop ends tick with zero wage payment due to owner insolvency.
2. Merchant cannot overspend across multiple facilities.
3. Regression test passes for multi-facility single-owner budget edge case.

## Problem 2: Labor Deadlock Without `wage_ema` Seed

### Problem

Labor skills are currently derived from `settlement.wage_ema.keys()`. If empty, labor phase exits early and settlement can never enter labor dynamics.

### Proposed Fix

1. Build labor skill universe from runtime state:
1. Pop skills in settlement.
2. Recipe-required skills for local facilities.
3. Existing `wage_ema` keys (if any).
2. Initialize missing wage EMA entries with deterministic baseline value.
3. Keep deterministic skill iteration order.

### Exit Criteria

1. Fresh settlement with skilled pops/facilities hires without manual wage EMA seeding.
2. New regression test passes.

## Problem 3: Settlement-Order Dependence

### Problem

World tick currently runs labor, production, market, and mortality for each settlement in sequence. Shared merchant state is mutated by early settlements before later settlements clear, creating arbitrary settlement-ID priority effects.

### Proposed Fix

Refactor to phase-wide orchestration:

1. Run labor for all settlements.
2. Run production for all settlements.
3. Run market for all settlements.
4. Run mortality for all settlements.

Within each phase, keep deterministic settlement ordering.

### Exit Criteria

1. Settlement insertion order no longer grants first-processed priority in shared-owner edge cases.
2. Aggregate outcomes are invariant (or within tolerance where stochastic effects apply).
3. Regression test comparing reversed settlement creation order passes.

## Problem 4: Arbitrary MVP Price Source

### Problem

Labor MVP currently uses `settlement.price_ema.values().next()`, which is unrelated to facility outputs and depends on hash iteration order.

### Proposed Fix

1. Replace single arbitrary output price with facility-specific output valuation.
2. Compute value from relevant recipe output goods only.
3. Use local `price_ema` per output good (with deterministic fallback).
4. Remove any dependence on unrelated `price_ema` entries.

### Exit Criteria

1. Adding/removing unrelated goods in `price_ema` does not change labor bids for a facility.
2. Regression test passes.

## Test Plan

Add the following tests before implementation work:

1. `labor_output_price_uses_relevant_goods_only`
2. `labor_bootstraps_without_seeded_wage_ema`
3. `no_unpaid_employed_pops`
4. `settlement_order_invariance_for_shared_merchant`

Then run:

1. `cargo test -q`
2. `cargo clippy -q --all-targets --all-features`

## Risk Notes

1. Problem 3 changes orchestration and may shift convergence traces; validate on existing scenario suite.
2. Problem 1 changes assignment feasibility semantics; expect some labor allocation differences in constrained scenarios.
3. Problems 2 and 4 should be low-risk correctness fixes and should land first.

## Suggested Commit Strategy

1. Commit A: tests only (red/expected fail).
2. Commit B: fix #4 + #2 (determinism/bootstrap).
3. Commit C: fix #1 (budget/payment consistency).
4. Commit D: fix #3 (phase orchestration), plus any threshold updates needed for stable tests.
