# Markets and Labor Spec (Current Runtime)

## Purpose

Describe current labor and goods market behavior as implemented, including subsistence reservation and external anchor interactions.

Primary references:

- `sim-core/src/labor/clearing.rs`
- `sim-core/src/labor/bidding.rs`
- `sim-core/src/labor/subsistence.rs`
- `sim-core/src/tick.rs`
- `sim-core/src/market/clearing.rs`
- `sim-core/src/external.rs`

## Goods Market

### Pop order generation

For each good:

1. Target stock = `desired_consumption_ema * BUFFER_TICKS`.
2. If below target: generate buy ladder over normalized price sweep.
3. If above target: generate sell ladder over excess stock.

Pop order behavior is stock-buffer and EMA-price driven, not optimization-based equilibrium solving.

### Merchant order generation

Merchant generates sell ladders only (in current behavior) from local stockpile:

1. Target is based on production EMA and fixed buffer.
2. Supply willingness rises when above target and/or price is attractive.
3. No explicit merchant buy ladder in current path.

### Clearing mechanism

Per good:

1. Evaluate candidate price points from order limits.
2. Compute volume at each price using budget-constrained demand and inventory-constrained supply.
3. Pick max-volume price (bias can favor sellers or buyers at ties).
4. Allocate fills proportionally within price levels.

Multi-good clearing:

1. Clear each good.
2. Check per-agent budget feasibility across tentative fills.
3. Relax by removing violating buy orders.
4. Iterate until feasible or max iterations.

### Fill semantics

For pop:

- Buy fill: `currency -= qty*price`, `stocks += qty`.
- Sell fill: `currency += qty*price`, `stocks -= qty`.

For merchant:

- Same currency symmetry.
- Goods apply to merchant stockpile at the settlement.

## Labor Market

### Current structure

Labor remains a bid+ask market in current code:

1. Facilities submit bids per skill (`max_wage`).
2. Pops submit asks per skill (`min_wage`).
3. Clear each skill in EMA-priority order.

This is not yet the v3 askless labor model.

### Wage formation

Labor market clears with seller-favoring bias in current implementation, causing clearing wages to sit at the employer bid edge when feasible.

Budget feasibility is enforced per facility via owner merchant currency.

### Adaptive facility bids

Each facility tracks per-skill adaptive bids:

1. If profitable slots unfilled: bid ratchets up (capped by marginal profitable MVP proxy).
2. If slots filled and global excess workers: bid ratchets down with floor.
3. Otherwise hold.

This is an adaptive controller around observed outcomes, not exact static optimization.

### Reservation wages

Baseline:

- Pop ask floor is `pop.min_wage`.

Optional override:

- Subsistence reservation ladder derived from in-kind fallback valuation:
  `reservation(pop_i) = q_i * grain_price_ref`.
- Rank-based fallback yields decline with crowding.

This creates a dynamic reservation curve tied to settlement conditions.

### Subsistence Mechanics

Two active mechanisms exist and can be enabled together:

1. In-kind subsistence injection to unemployed pops during settlement tick.
2. Subsistence-derived reservation asks during labor phase.

This differs from legacy settlement-org-only subsistence wage flow designs.

### External Anchor

Optional per-settlement outside market:

1. Adds import sell ladders and export buy ladders for anchored goods.
2. Pricing centered on world reference with spread + local frictions.
3. Finite per-tick depth with multiple tiers.
4. External fills counted into import/export stats.

Current topology behavior:

1. Any settlement can be configured for outside ladders.
2. There is no runtime-enforced "ports only" rule yet.
3. Inland settlements do not currently require merchant transport mediation to access outside liquidity.

Effects:

1. Softly bounds local prices via arbitrage-like outside opportunities.
2. Allows explicit currency/goods flow against external side.

## Operational Differences from Legacy Specs

1. No settlement-org market participant in active runtime path.
2. Labor not yet demand-side-only.
3. Pop demand budget is tied to `income_ema` and current currency, not wealth-target policy from v1/v2 docs.
4. External anchor is implemented as outside order ladders, not implicit price peg.
