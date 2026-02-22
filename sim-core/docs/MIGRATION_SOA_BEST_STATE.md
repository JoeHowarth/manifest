# Migration Plan: Settlement-Scoped SoA (Best-State, Breaking-OK)

## Purpose

Move the simulation to a settlement-owned SoA architecture with a clean API and no compatibility constraints.

This plan is optimized for:

1. Correct settlement scoping.
2. Lower borrow friction.
3. Practical migration risk control via phased delivery.
4. A final API that does not carry legacy ID shims.

## Design Principles

1. Settlement-local state is owned by `SettlementState`.
2. Public access uses typed handles, not raw map internals.
3. Structural refactors and behavior changes are separated.
4. Transitional compatibility layers are temporary and removed.
5. Determinism-sensitive paths are called out explicitly.

## End-State Data Model

## Key Types

```rust
use slotmap::{new_key_type, SecondaryMap, SlotMap};

new_key_type! { pub struct PopKey; }
new_key_type! { pub struct FacilityKey; }
```

These are the canonical runtime identities.

## Handles

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PopHandle {
    pub settlement: SettlementId,
    pub key: PopKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FacilityHandle {
    pub settlement: SettlementId,
    pub key: FacilityKey,
}
```

Handles solve lookup without a global reverse index in common code paths.

## SettlementState

```rust
pub struct SettlementState {
    pub id: SettlementId,
    pub info: Settlement,

    pub pops: SlotMap<PopKey, Pop>,
    pub facilities: SlotMap<FacilityKey, Facility>,

    pub price_ema: HashMap<GoodId, Price>,
    pub wage_ema: HashMap<SkillId, Price>,
    pub facility_bid_states: SecondaryMap<FacilityKey, FacilityBidState>,
    pub subsistence_queue: Vec<PopKey>,
    pub depth_multipliers: HashMap<GoodId, f64>,

    // Owner cache to make merchants_at(sid) O(number_of_owners), not O(number_of_facilities)
    pub owner_facility_counts: HashMap<MerchantId, u32>,
}
```

`owner_facility_counts` is updated on facility add/remove and provides cheap `merchants_at` queries.
`facility_bid_states` is a full sidecar map (all facilities have bid state).

## World

```rust
pub struct World {
    pub tick: u64,
    pub settlements: HashMap<SettlementId, SettlementState>,
    pub routes: Vec<Route>,

    pub merchants: HashMap<MerchantId, MerchantAgent>,

    pub external_market: Option<ExternalMarketConfig>,
    pub subsistence_reservation: Option<SubsistenceReservationConfig>,
    pub mortality_grace_ticks: u64,

    pub outside_flow_totals: OutsideFlowTotals,
    pub stock_flow_history: Vec<TickStockFlow>,

    next_settlement_id: u32,
    next_agent_id: u32,
}
```

## Agent/Facility Cross-References (Explicit)

This is the target type migration:

1. `Pop.employed_at: Option<FacilityId>` -> `Option<FacilityKey>`.
2. `MerchantAgent.facility_ids: HashSet<FacilityId>` -> `owned_facilities: HashSet<FacilityHandle>`.
3. `FacilityBidState` world keying by `FacilityId` -> settlement-local keying by `FacilityKey`.
4. `Pop.home_settlement: SettlementId` -> removed (redundant; settlement comes from `PopHandle`/context).
5. `Pop.id` and `Facility.id` fields -> removed (identity is key/handle).
6. `Facility.settlement` -> removed (owned by settlement storage).

Notes:

- Employment is local by construction, so `FacilityKey` is sufficient for `Pop.employed_at`.
- Merchant ownership uses `FacilityHandle` so settlement is always explicit.
- If a debug invariant is desired, add assertions when applying assignments rather than persisting redundant fields.

## Identity and Instrumentation IDs

No separate telemetry ID layer is needed.

`PopKey` and `FacilityKey` are converted directly to `u64` for logs/Polars:

```rust
pub fn pop_key_u64(k: PopKey) -> u64 {
    k.data().as_ffi()
}

pub fn facility_key_u64(k: FacilityKey) -> u64 {
    k.data().as_ffi()
}
```

Properties:

1. Unique for live keys.
2. Generation-aware (slot reuse gets new identity).
3. Not stable across runs.

Consequence:

- Any baseline/report that compares entity identity across runs must not rely on these IDs. Use aggregate or distributional comparisons for cross-run baseline checks.

## API Shape

## Construction and Access

1. `add_settlement(...) -> SettlementId`
2. `add_pop(sid) -> PopHandle`
3. `add_facility(kind, sid, owner) -> FacilityHandle`
4. `pop(handle) -> Option<&Pop>`
5. `pop_mut(handle) -> Option<&mut Pop>`
6. `facility(handle) -> Option<&Facility>`
7. `facility_mut(handle) -> Option<&mut Facility>`

## Iteration and Queries

1. `pops_at(sid) -> impl Iterator<Item = (PopKey, &Pop)>`
2. `facilities_at(sid) -> impl Iterator<Item = (FacilityKey, &Facility)>`
3. `merchants_at(sid) -> impl Iterator<Item = MerchantId>` derived from `owner_facility_counts.keys()`.

No direct public exposure of internal world maps.

## Ownership Update Rules

These are mandatory operations, not optional details:

1. `add_facility` must:
- insert into `SettlementState.facilities`
- increment `SettlementState.owner_facility_counts[owner]`
- insert `FacilityHandle` into `MerchantAgent.owned_facilities`

2. Facility removal must:
- remove from `SettlementState.facilities`
- decrement/remove `owner_facility_counts`
- remove `FacilityHandle` from `MerchantAgent.owned_facilities`
- clear any `Pop.employed_at` references to that facility key

## Tick Pipeline

```text
run_tick:
  tick += 1
  capture pre snapshot

  take merchants map once

  for each settlement state:
    run_labor
    run_production
    run_subsistence
    run_consumption
    run_market
    update_price_ema
    run_mortality

  restore merchants map

  capture post snapshot
```

## Merchant Mutability Strategy

## Strategy Chosen for This Migration

Use per-settlement merchant extraction/reinsert from the tick-local merchant map.

Why now:

1. Minimal conceptual change from current mechanics.
2. Keeps migration scope bounded while removing pop/facility extract/reinsert.
3. Merchant cardinality is typically much smaller than pop cardinality.

## Delta Strategy (Deferred by Design)

Settlement-returned merchant deltas are cleaner for parallel execution, but increase immediate complexity (new delta types, merge semantics, conflict rules).

Planned compatibility seam:

- Settlement phase methods should be structured so merchant mutation is isolated behind a narrow local interface.
- This keeps replacement with delta-application feasible later without rewriting settlement economics.

## Behavior Scope by Phase

1. Structural phases are "behavior-preserving within tolerance".
2. Labor migration is an intentional behavior change.
3. Any deterministic order-sensitive drift is tracked and bounded.

## Determinism and Iteration Order

SlotMap iteration order differs from current global `HashMap` usage.

For order-sensitive logic, use explicit ordering:

1. Subsistence queue initialization must use sorted key order (`pop_key_u64`) where needed.
2. Any logic deriving deterministic order IDs from iteration must sort inputs first.
3. Tests should validate invariants/metrics, not incidental internal iteration order.

## Refactor Sequence (Commit-Oriented)

## Phase 0: Guardrails and Test Access API

Goal: reduce churn risk before storage migration.

1. Add read/query helpers used by tests and diagnostics.
2. Migrate tests away from direct field pokes where possible.
3. Keep simulation behavior unchanged.

Exit criteria:

1. Tests compile via query API.
2. No material metric drift.

## Phase 1: Introduce SettlementState + Keys + Handles Scaffold

Goal: create the new storage and identity substrate.

1. Define `PopKey`, `FacilityKey`, handles, and key->u64 helpers.
2. Add `SettlementState` with slotmaps and local market/labor fields.
3. Add `owner_facility_counts` cache.
4. Keep adapters so existing logic still runs.

Exit criteria:

1. Build passes.
2. Behavior preserved within tolerance.

## Phase 2: Move Ownership to SettlementState (Behavior-Preserving Within Tolerance)

1. Move pops/facilities/prices/wages/bids/subsistence/depth into `SettlementState`.
2. Remove `Settlement.pop_ids` and world-global pop/facility maps.
3. Migrate `add_pop`/`add_facility` and ownership update rules.
4. Convert cross-references (`employed_at`, merchant ownership sets).

Exit criteria:

1. All tests compile with new storage model.
2. Invariants hold.
3. Drift is within defined tolerances.

## Phase 3: Labor Migration (Intentional Behavior Change)

1. Execute labor clearing per settlement only.
2. Use settlement-local asks/bids/wage EMA/bid states.
3. Explicitly initialize per-settlement `wage_ema` policy.

Exit criteria:

1. No cross-settlement assignment possible.
2. Labor invariants and targeted economic checks pass.

## Phase 4: Production + Market Method Migration

1. Run production inside settlement loop.
2. Move `run_settlement_tick` internals onto settlement methods.
3. Remove deprecated `run_market_tick`.

Exit criteria:

1. End-to-end tick is settlement-method driven.
2. External anchor and market tests pass.

## Phase 5: Mortality Migration

1. Run mortality per settlement.
2. Keep inheritance/removal local to settlement arenas.

Exit criteria:

1. Population and accounting invariants pass.

## Phase 6: Delete Transitional Surfaces

1. Remove remaining legacy IDs and wrappers.
2. Remove adapter glue and dead code.
3. Finalize API surface.

Exit criteria:

1. No duplicate old/new paths.
2. Final API matches this document.

## Validation Strategy

At each phase:

1. Run invariant tests (currency, stock non-negativity, labor accounting).
2. Run integration tests (single and multi-settlement scenarios).
3. Run stability/convergence suites with phase-aware comparisons.
4. Verify instrumentation schema and joins (`pop_id`, `facility_id` as key-derived `u64`).
5. Verify baseline methodology does not assume cross-run entity ID stability.

## Known Model Deltas

Expected and acceptable:

1. Wage/price dynamics after per-settlement labor.
2. Convergence baseline shifts.
3. Secondary mortality effects from corrected local labor/market interactions.

Treat these as model evolution, not regressions, when invariants remain satisfied.

## Non-Goals in This Migration

1. Rayon settlement parallelism.
2. Merchant state sharding.
3. Fixed-size pop stock arrays.

## Final End-State Checklist

1. No global `World.pops` map.
2. No global `World.facilities` map.
3. No `Settlement.pop_ids`.
4. No global labor clearing.
5. No pop extract/reinsert ceremony.
6. `run_market_tick` removed.
7. Merchant ownership + settlement owner cache maintained correctly.
8. Instrumentation uses key-derived `u64` IDs.
9. Tests and invariants pass on new API.
