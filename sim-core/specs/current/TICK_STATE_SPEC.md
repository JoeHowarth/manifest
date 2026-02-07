# Tick and State Spec (Current Runtime)

## Purpose

Define the exact world tick sequence and state transitions that occur in current code.

Primary implementation references:

- `sim-core/src/world.rs`
- `sim-core/src/tick.rs`
- `sim-core/src/consumption/greedy.rs`
- `sim-core/src/mortality.rs`

## Canonical Tick Order

Entry point: `World::run_tick(...)`.

Order:

1. Labor phase (`run_labor_phase`).
2. Production phase (`run_production_phase`).
3. Settlement phase (`run_settlement_tick`) for each settlement.
4. Mortality phase (`run_mortality_phase`).

## Phase 1: Labor

### Inputs

- Pop skills and reservation baseline (`pop.skills`, `pop.min_wage`).
- Wage EMA by skill (`world.wage_ema`).
- Facility ownership/capacity/worker maps.
- Merchant currency for labor budgets.
- Optional subsistence reservation config.

### Operations

1. Facilities generate labor bids (adaptive capped by simplified MVP proxy).
2. Pops generate asks by skill.
3. Optional subsistence reservation ladder can override per-pop ask floor.
4. Labor markets clear by skill with budget checks.
5. Wage EMA updates from clearing outcomes.
6. Assignments applied; merchant currency transfers to pop currency.
7. `income_ema` updated via `record_income`.

### Outputs

- Pop fields updated: `employed_at`, `currency`, `income_ema`.
- Facility worker maps rewritten from assignments.
- Merchant currency reduced by wage payments.

## Phase 2: Production

### Inputs

- Facility workers/capacity/recipe priorities.
- Merchant settlement stockpiles.
- Settlement resource quality multipliers.

### Operations

1. Allocate recipe runs under capacity/worker/input constraints.
2. Execute production: consume inputs, add outputs to merchant stockpile.
3. Update merchant production EMA by settlement/good.

### Outputs

- Merchant inventories change; pop inventories do not change directly.

## Phase 3: Settlement Tick

Executed independently per settlement.

### 3a) Optional in-kind subsistence

- If enabled, unemployed pops receive grain directly into `pop.stocks`.
- Yield decreases by rank with crowding.

### 3b) Consumption

Per pop:

1. Clear `need_satisfaction` for current tick.
2. Compute two consumption passes:
   - Discovery pass (budgeted by `income_ema`) to infer desired demand.
   - Actual pass (stock-only with biased virtual prices) to consume goods.
3. Subtract actual consumption from stock.
4. Update `desired_consumption_ema` by smoothing desired quantities.

### 3c) Order generation

Sources:

1. Pop order ladders from stock-vs-target and price-vs-EMA logic.
2. Merchant supply ladders from stock-vs-target and price-vs-EMA logic.
3. Optional outside import/export ladders from external anchor config.

Topology note:

- Current runtime does not enforce a "port settlements only" outside-access constraint.
- Port-gated external access with merchant-mediated inland propagation is a target design, not yet current behavior.

### 3d) Budgets and inventory constraints

- Pop buy budget: `min(income_ema, currency)`.
- Merchant buy budget: `merchant.currency`.
- Seller inventory caps enforced from current stockpiles.

### 3e) Market clearing and fill application

1. Clear all goods using iterative multi-market call auction.
2. Apply fills to pops and merchants (currency + stocks).
3. Track external import/export fills when outside agents trade.

### 3f) Price EMA update

- Clearing prices smooth into settlement price EMA with 0.7/0.3 blend.

## Phase 4: Mortality and Growth

Mortality logic uses `need_satisfaction["food"]`.

### Death probability

- Zero at satisfaction >= 0.9.
- Quadratic increase below 0.9.
- Capped at 0.99 at extreme deficit.

### Growth probability

- Zero at satisfaction <= 1.0.
- Linear ramp above 1.0.
- Capped by small max growth probability.

### Demographic updates

- Dead pops removed from world + settlement references.
- Growth clones parent traits into new pop.
- Child currency comes from parent split; no currency minting.

## Pop State Transition Summary

Fields most frequently changed each tick:

1. `employed_at`: reassigned in labor phase.
2. `income_ema`: updated from wage (or zero if unemployed).
3. `currency`: wage inflow and market buy/sell flows.
4. `stocks`: subsistence inflow, consumption outflow, market fill flows.
5. `need_satisfaction`: recomputed each settlement tick.
6. `desired_consumption_ema`: smoothed from discovery demand.

## Key Modeling Note

Current runtime uses a mixed control style:

1. Labor-side income constraints.
2. Stock-buffer-based goods demand/supply heuristics.
3. Stochastic demography from realized food satisfaction.

This means equilibrium is emergent from coupled controllers rather than from one closed-form policy layer.
