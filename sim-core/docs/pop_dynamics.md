# Pop Dynamics (Implementation Ground Truth)

This document describes how `Pop` behaves in the current implementation.
It is intentionally code-first and may differ from older design specs.

## Scope

- Atomic unit: `Pop` is the basic population abstraction in this codebase.
- Source of truth: runtime behavior in `sim-core/src`.
- This doc focuses on:
1. Tick-order data flow.
2. Pop state transitions.
3. Feedback loops.
4. Invariants and tests.
5. Parameter defaults.
6. Spec drift.

## Pop State Vector

`Pop` fields in `sim-core/src/agents/pop.rs`:

- `id`, `home_settlement`.
- `currency`.
- `stocks: HashMap<GoodId, Quantity>`.
- `desired_consumption_ema: HashMap<GoodId, Quantity>`.
- `need_satisfaction: HashMap<String, f64>`.
- `income_ema`.
- `skills`.
- `min_wage`.
- `employed_at`.

Default constructor values in `sim-core/src/agents/pop.rs`:

- `currency = 1000.0`.
- `income_ema = 100.0`.
- `min_wage = 1.0`.
- Empty stocks/skills/EMAs/satisfaction.

## Canonical Tick Order

World tick entrypoint: `World::run_tick` in `sim-core/src/world.rs`.

Order:
1. Labor phase (`run_labor_phase`).
2. Production phase (`run_production_phase`).
3. Per-settlement tick (`run_settlement_tick`):
   consumption, order generation, market clearing, fill application, price EMA.
4. Mortality phase (`run_mortality_phase`).

## Pop Data Flow by Phase

### 1) Labor Phase

Code: `sim-core/src/world.rs`.

Reads:
- `pop.skills`.
- `pop.min_wage` unless subsistence reservation override is enabled.

Writes:
- `pop.employed_at` reset then set from assignments.
- `pop.currency` (+wage when hired).
- `pop.income_ema` via `record_income`.

Mechanics:
- Pops submit asks per skill (`generate_pop_asks_with_min_wage`).
- Reservation can be overridden by subsistence ladder:
  `build_subsistence_reservation_ladder` in `sim-core/src/labor/subsistence.rs`.
- Facilities bid and market clears by skill (`clear_labor_markets`).
- Merchant pays wage directly to pop (facility treasury is not the active path yet).

### 2) Production Phase

Code: `sim-core/src/world.rs`, `sim-core/src/production/execute.rs`.

Direct pop writes: none.

Indirect effect:
- Changes merchant settlement stockpiles, which changes future goods supply to pops.

### 3) Settlement Tick (Consumption + Goods Market)

Code: `sim-core/src/tick.rs`.

#### 3a) Optional in-kind subsistence injection

- If enabled, unemployed pops receive grain directly into `pop.stocks`.
- Ranked yields come from `ranked_subsistence_yields`:
  `q(rank) = q_max / (1 + crowding_alpha * (rank - 1))`.

#### 3b) Consumption

For each pop:
- `need_satisfaction.clear()`.
- `compute_consumption(...)` runs two passes in `sim-core/src/consumption/greedy.rs`:
1. Discovery pass (budgeted by `income_ema`) -> `desired`.
2. Actual pass (from stockpile, no currency budget) -> `actual`.

Writes:
- `pop.need_satisfaction` from actual pass.
- `pop.stocks -= actual`.
- `pop.desired_consumption_ema = 0.8 * old + 0.2 * desired`.

#### 3c) Goods order generation

`generate_demand_curve_orders` in `sim-core/src/tick.rs`.

For each good:
- `target = desired_consumption_ema * BUFFER_TICKS`.
- If `stock < target`: pop posts buy ladder.
- Else: pop posts sell ladder from excess.

Demand shape helpers:
- `qty_norm(norm_p, norm_c) = clamp(shortfall * (0.3 + 0.7 * (1 - norm_p)), 0, 1)`.
- `qty_sell(norm_p, norm_c) = qty_norm(1 / norm_p, 1 / norm_c)`.

#### 3d) Budgets and inventory constraints

- Buyer budget for pop: `min(income_ema, currency)`.
- Seller inventory for pop: current stock by good.
- Auction enforces both constraints.

#### 3e) Clearing and fills

- Goods clear via `clear_multi_market` / `clear_single_market`.
- Fill application for pops (`market::apply_fill`):
  - buy: `currency -= qty * price`, `stocks += qty`.
  - sell: `currency += qty * price`, `stocks -= qty`.

#### 3f) Price EMA

- Settlement price EMA update: `0.7 * old + 0.3 * clearing_price`.

### 4) Mortality / Growth

Code: `sim-core/src/world.rs`, `sim-core/src/mortality.rs`.

Input:
- `food_satisfaction = need_satisfaction["food"]` (exact key match).

Death:
- `food_satisfaction >= 0.9 -> p_death = 0`.
- Below 0.9: quadratic ramp, capped at 0.99.

Growth:
- `food_satisfaction <= 1.0 -> p_growth = 0`.
- Above 1.0: linear ramp to max 0.02 by 1.25 satisfaction.

On growth:
- Child pop is cloned from parent traits.
- Parent/child split parent currency 50/50 (no new currency minted).
- Child stocks cleared at spawn.

On death:
- Pop removed from world + settlement pop list.
- Employment/accounting structures updated.

## Core Feedback Loops

### 1) Wage -> Income EMA -> Market Budget -> Food Satisfaction -> Demography

1. Labor assignment sets wage cash flow.
2. `income_ema` updates.
3. Pop buy budget uses `min(income_ema, currency)`.
4. Purchases affect stock and then consumption satisfaction.
5. Food satisfaction drives death/growth probability.
6. Population level shifts labor supply and demand next tick.

### 2) Stock Buffer -> Order Curves -> Price -> Future Orders

1. `desired_consumption_ema` sets target stock.
2. Gap vs target shapes buy/sell ladder.
3. Clearing price updates EMA.
4. Next tick order ladder is generated off new EMA and updated stock.

### 3) Subsistence Outside Option -> Reservation Wages -> Hiring

When subsistence reservation is enabled:
- Reservation ask for each pop is valued from ranked in-kind fallback.
- Higher fallback -> higher ask -> lower formal hiring at low wages.
- Lower fallback under crowding -> lower asks -> easier hiring.

### 4) External Anchor -> Local Price Band -> Import/Export Flows

When enabled per settlement:
- Outside import/export ladders are added around world reference price plus frictions.
- Imports/exports adjust local goods and local currency through normal fills.
- Flow totals tracked in `outside_flow_totals`.

## Utility and Satisfaction Semantics

Need utility curves in `sim-core/src/needs.rs`.

Current subsistence utility:
- Strong marginal utility below requirement.
- Small positive surplus tail from 1.0 to 1.25 satisfaction ratio.
- Zero marginal utility above 1.25.

Implication:
- Food can still carry positive marginal value slightly above survival.
- Supports limited growth channel when food is abundant.

## Key Default Parameters (Current Code)

Demand/market:
- `BUFFER_TICKS = 5.0` in `sim-core/src/tick.rs`.
- Price sweep for pop ladders: `0.6..1.4`, `9` points.
- Settlement price EMA alpha: `0.3` new.

Income:
- `income_ema = 0.7 * old + 0.3 * wage` in `sim-core/src/agents/pop.rs`.

Mortality:
- Death-free threshold: `0.9`.
- Surplus growth cap point: `1.25`.
- Max growth probability: `0.02`.

Subsistence reservation config defaults:
- `grain_good = 1`.
- `q_max = 2.0`.
- `crowding_alpha = 0.02`.
- `default_grain_price = 10.0`.

External anchor defaults:
- `world_price = 10.0`.
- `spread_bps = 500`.
- `depth_per_pop = 0.5`.
- `tiers = 9`.
- `tier_step_bps = 300`.
- Settlement anchor is off unless enabled.

## Invariants and Test Coverage

High-signal checks in `sim-core/tests/invariants.rs` and `sim-core/tests/properties.rs`:

- Currency conservation under growth (child funded by parent split).
- Pop cannot oversell inventory in settlement tick.
- Labor assignment accounting consistency:
  employed-pop count == facility worker totals.
- External flow accounting consistency:
  local currency delta matches exports - imports value.
- Subsistence ranking monotonicity:
  earlier-ranked pops receive more fallback output.
- No negative stock/currency and bounded/finite price EMA in property tests.

Convergence characterization in `sim-core/tests/convergence.rs`:
- Strict convergence threshold set (price std, pop slope, stock slope, employment, food sat).
- Weak stability threshold set (looser and used in gating for sweep scenarios).
- Includes scenario sweeps plus non-gating stress characterization.

## Known Spec Drift (Important)

Compared with `sim-core/specs/v2` and `sim-core/specs/labor-market-v3.md`:

1. Labor market:
   current code still has pop asks; labor-v3 spec proposes demand-side-only.
2. Subsistence:
   current code can inject in-kind goods directly and/or convert fallback to reservation asks;
   v2 docs emphasize settlement-org wage flow model.
3. Tick order:
   current implementation runs labor then production then settlement consumption/market;
   some specs describe different sequencing.
4. Settlement-org model:
   current runtime model is pop+merchant centric; full settlement-org treasury behavior is not active.
5. Mortality keying:
   mortality depends on need id `"food"` specifically.
6. Multi-good social model:
   specs discuss provisions/cloth and richer wealth behavior; implementation is generic good/need mapping with current tests focused heavily on grain/food.

## Practical Guidance

When changing pop dynamics, treat these as coupled:
1. Labor reservation and wage adjustment.
2. Consumption utility and stock-buffer behavior.
3. Budget constraints and price discovery.
4. Mortality/growth probabilities.
5. External anchor strength and depth.

Small local tweaks in one layer can destabilize the full loop unless checked against convergence and invariant tests together.
