# Labor Market v3: Adaptive Facility Bidding

## Summary

Replace the double auction (population asks + facility bids) with a demand-side-only auction. Workers don't negotiate — they take the best available job. Facilities compete by bidding adaptively. Subsistence farming is the outside option that sets the wage floor.

## Core Principles

1. **Workers are price-takers.** No worker turns down a job paying above subsistence to go farm. There is no ask side.
2. **Facilities are price-setters.** They bid for labor based on marginal value product (MVP), competing against each other.
3. **Subsistence is the outside option.** Any worker not hired by a facility goes to subsistence at wage = `SUBSISTENCE_WAGE` (20). This is the absolute floor.
4. **Min profit margin.** Facilities never bid above `MVP * (1 - MIN_MARGIN)`. With `MIN_MARGIN = 0.05`, a facility with MVP=30 bids at most 28. Facilities always retain some surplus.
5. **Orgs with MVP < SUBSISTENCE_WAGE don't bid.** If a worker produces less value than subsistence, the facility can't profitably employ them.

## MVP and the Demand Curve

Each facility generates a stepped demand curve from its diminishing returns:

```
recipe.marginal_output(workers, optimal_workforce) * (output_price - input_cost)
```

For a facility with `optimal_workforce = 20`:
- Workers 0-20: MVP = `output_per_worker * net_price` (constant, at/below optimal)
- Workers 21+: MVP = `output_per_worker * e^(-taper * excess/optimal) * net_price` (decreasing)

The facility submits multiple bid tiers. Each tier specifies a quantity and max price:

```
Tier 0: 20 workers @ min(MVP(10), MVP * 0.95)   — optimal capacity, mid-fill MVP
Tier 1:  5 workers @ min(MVP(22), MVP * 0.95)    — first diminishing batch
Tier 2:  5 workers @ min(MVP(27), MVP * 0.95)    — second diminishing batch
...until MVP drops below SUBSISTENCE_WAGE
```

## Clearing Algorithm

Each tick, per settlement:

### 1. Collect bids
All non-subsistence facilities generate bid tiers as above.

### 2. Sort bids descending by price
Merge all tiers from all facilities into a single list, sorted highest price first.

### 3. Fill from top down
Available workers = population * labor_force_rate.

Walk the sorted bid list. Assign workers to each tier until workers run out.

### 4. Handle the marginal tier (pro-rata)
When remaining workers < remaining slots at the current price level, distribute proportionally among all bids at that price:

```
facility_share = facility_slots_at_this_price / total_slots_at_this_price
workers_assigned = remaining_workers * facility_share
```

### 5. Set the clearing wage
The clearing wage = the price of the lowest tier that received any workers (the marginal bid).

If no facility bids clear (all MVPs below subsistence), wage = SUBSISTENCE_WAGE.

### 6. Execute payments
- Each facility pays `workers_assigned * clearing_wage` from its org treasury.
- Workers receive wages: `total_labor_hired * clearing_wage` added to population wealth.
- Unhired workers go to subsistence (handled separately, paid SUBSISTENCE_WAGE by settlement org).

Note: All hired workers pay the same clearing wage (uniform pricing). This removes incentive for facilities to shade bids — bidding true value is optimal.

### 7. Record market state
Update `labor_market.wage`, `labor_market.supply`, `labor_market.demand` for UI and diagnostics.

## Adaptive Bidding

Facilities do NOT bid their theoretical MVP directly. MVP depends on output prices, which are themselves moving. Bidding raw MVP causes oscillation: high grain price → high farm MVP → overhire → grain glut → price crash → mass layoffs → repeat.

Instead, facilities bid adaptively around the last clearing wage:

### Bid Adjustment Rule

Each facility tracks the last clearing wage for its settlement. Each tick:

```
if underfilled last tick:
    bid = min(last_bid + RATCHET_UP, mvp * (1 - MIN_MARGIN))
if fully filled last tick:
    bid = max(last_bid - RATCHET_DOWN, SUBSISTENCE_WAGE)
```

Constants:
- `RATCHET_UP = 2.0` — bid increment when competing for scarce workers
- `RATCHET_DOWN = 1.0` — bid decrement when overpaying (slower descent = stickier wages)
- `MIN_MARGIN = 0.05` — never bid above 95% of MVP
- Floor: `SUBSISTENCE_WAGE` (20) — never bid below the outside option

### Starting bid

New facilities (or tick 0) start at `SUBSISTENCE_WAGE`. They discover the market wage by ratcheting up.

### Why Adaptive?

1. **Stability** — bids move gradually, no oscillation from output price shocks
2. **Discovery** — facilities find the actual clearing wage through competition, not through a theoretical calc that assumes perfect price knowledge
3. **Realism** — orgs learn what the market will bear over time

### Convergence

With RATCHET_UP=2 and RATCHET_DOWN=1, a facility starting at 20 reaches a clearing wage of 28 in ~4 ticks. The asymmetric rates (up faster, down slower) create realistically sticky wages — hiring ramps up quickly but layoffs/wage cuts happen gradually.

## Worked Example

**Setup:** 40 workers, subsistence = 20, min margin = 5%

Facility A: 20w optimal @ MVP=40, 5w diminishing @ MVP=30, 5w tail @ MVP=20
Facility B: 20w optimal @ MVP=30, 5w diminishing @ MVP=25, 5w tail @ MVP=20

**Max bids (after 5% margin):**
- A optimal: 20w @ 38
- A diminishing: 5w @ 28
- B optimal: 20w @ 28
- B diminishing: 5w @ 23
- A tail: 5w @ 19 → below subsistence, dropped
- B tail: 5w @ 19 → below subsistence, dropped

**Sorted bids:** 20@38, 25@28 (A's 5 + B's 20), 5@23

**Filling 40 workers:**
1. 20 workers → A optimal @ 38. Remaining: 20 workers.
2. 25 slots @ 28, 20 workers remain. Pro-rata: A gets 5/25*20 = 4, B gets 20/25*20 = 16.
3. Stop. 0 workers remain.

**Result:**
- Clearing wage: **28** (marginal tier price)
- A: 24 workers (20 optimal + 4 diminishing), pays 24 * 28 = 672
- B: 16 workers (of 20 optimal), pays 16 * 28 = 448
- Subsistence: 0 workers
- A profit: 20*40 + 4*30 - 672 = 800 + 120 - 672 = 248
- B profit: 16*30 - 448 = 480 - 448 = 32

## Surplus Labor Example

**Setup:** 100 workers, same facilities.

50 slots available (after dropping MVP < 20 tiers). 100 workers.

All 50 slots fill. 50 workers go to subsistence. Marginal tier is the lowest bid that cleared (23, B's diminishing). Clearing wage = 23.

Both facilities fully staffed. 50 surplus workers farm subsistence at wage 20.

## Migration from Current System

### Remove
- `generate_population_labor_asks()` — no ask side
- Wealth factor / reservation wage logic
- Midpoint clearing price formula

### Modify
- `generate_facility_labor_bids()` — use tiered MVP bids with min margin
- `run_labor_auction_v2()` → `run_labor_market_v3()` — new clearing algorithm
- `clear_market()` not used for labor — labor uses its own fill-from-top algorithm

### Keep
- `LaborMarket` struct (supply, demand, wage tracking)
- Subsistence production (unchanged)
- Facility diminishing returns via `Recipe::marginal_output()`
