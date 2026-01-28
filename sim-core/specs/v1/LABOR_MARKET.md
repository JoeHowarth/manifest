# Labor Market Specification

## Overview

The labor market matches workers (from settlement population) with jobs (at facilities). Unlike a simple "everyone works, split proportionally" model, this system uses supply and demand curves that respond to economic conditions.

**Key principles:**
- Workers have **reservation wages** — minimum pay they'll accept, influenced by wealth
- Facilities have **marginal value product (MVP)** per worker slot — what each additional worker is worth
- The market clears where supply meets demand
- Profitable facilities naturally outcompete unprofitable ones for labor

## Worker Supply Curve

### Reservation Wages

Each potential worker has a minimum wage they'll accept. This depends on:

1. **Subsistence floor** — Nobody works for less than they could earn foraging/informal work
2. **Wealth level** — Wealthy people demand higher wages; poor people accept lower wages
3. **Individual variation** — Not everyone has the same reservation (creates a curve, not a cliff)

### Model

Sort the labor pool by reservation wage to create a supply curve:

```
For worker at percentile P (0 = poorest, 1 = wealthiest):

  reservation_wage(P) = SUBSISTENCE_WAGE * (1 + wealth_factor * P^elasticity)

Where:
  SUBSISTENCE_WAGE = 20 (the anchor - what subsistence farming pays)
  wealth_factor = how much wealth shifts the curve (e.g., 1.0 at target wealth, 2.0 when wealthy)
  elasticity = shape of distribution (e.g., 0.5 for concave curve)
```

**Effect of aggregate wealth:**

When population is wealthy (`avg_wealth > target_wealth`):
- The entire curve shifts right (higher reservation wages across the board)
- `wealth_factor` increases
- Fewer workers willing to work at any given wage

When population is poor (`avg_wealth < target_wealth`):
- Curve shifts left (lower reservation wages)
- `wealth_factor` decreases
- More workers willing to work at any given wage

### Supply Function

At wage W, labor supply is the count of workers whose reservation wage ≤ W:

```
labor_supply(W) = population * labor_force_rate * CDF(W)

Where:
  labor_force_rate = fraction of population able to work (e.g., 0.6)
  CDF(W) = cumulative distribution of reservation wages up to W
```

This produces an upward-sloping supply curve: higher wages → more workers willing to work.

## Facility Demand Curve

### Marginal Value Product (MVP)

Each worker slot at a facility has a value based on:
- How much additional output that worker produces
- The market price of that output
- The cost of inputs consumed

```
MVP(nth worker) = marginal_output(n) * output_price - marginal_input_cost(n)
```

### Diminishing Returns

Facilities have an optimal workforce. Workers beyond optimal contribute less:

```
For facility with optimal_workforce = N:

  If workers ≤ N:
    marginal_output(n) = base_output / N  (linear up to optimal)

  If workers > N:
    marginal_output(n) = (base_output / N) * decay^(n - N)
    Where decay < 1 (e.g., 0.7 per extra worker)
```

### Demand Function

A facility will hire workers as long as MVP > wage:

```
labor_demand_facility(W) = count of slots where MVP(slot) >= W
```

Total settlement demand is sum across all facilities:

```
labor_demand(W) = Σ labor_demand_facility(W) for all facilities
```

This produces a downward-sloping demand curve: higher wages → fewer slots worth filling.

### Input Availability Constraint

MVP calculation should account for input availability:

```
If facility lacks inputs:
  effective_MVP = 0 (can't produce without inputs)

If facility has partial inputs:
  effective_MVP = MVP * (available_inputs / required_inputs)
```

This prevents facilities from demanding labor when they can't actually produce.

## Subsistence Farming: The Anchor and Floor

Every settlement has a **Subsistence Farm** — a communal facility that provides the economic anchor and unemployment backstop.

### Purpose

1. **Wage anchor**: Fixed wage of 20 provides a numeraire for the economy
2. **Price anchor**: Provision prices soft-anchor around what subsistence wages can afford
3. **Unemployment backstop**: Workers always have fallback employment
4. **Population limiter**: Diminishing returns naturally cap sustainable population

### Properties

```
Subsistence Farm:
  - Owner: Settlement (not an Org)
  - Wage: 20 (FIXED, does not participate in auction)
  - Capacity: ~50% of settlement population
  - Output: Provisions
  - Does not require natural resources (available everywhere)
```

### Diminishing Returns

The key mechanic: each additional worker produces LESS than the previous one.

```
Worker slots 1-100:    produce 1.2 provisions each (net positive)
Worker slots 101-300:  produce 0.8 provisions each (net negative)
Worker slots 300+:     produce 0.4 provisions each (clearly unsustainable)
```

A worker earning 20 needs to buy ~1 provision to survive. So:
- First 100 workers: sustainable, small surplus
- Workers 101-300: slowly depleting wealth/stockpile
- Beyond 300: rapid depletion

### The Poverty Equilibrium

If a settlement has only subsistence farming:

1. Everyone works subsistence, earns 20 each
2. Total provisions produced < total workers (diminishing returns)
3. Demand > supply → price rises
4. Workers can't afford enough food at high prices
5. Wealth depletes trying to buy scarce food
6. Eventually: spending constrained to actual income
7. Population contracts until production matches consumption
8. Stable poverty: small population, low wealth, provision price ~20

**This is the floor, not the goal.** Better facilities with higher productivity are the path to prosperity.

### Integration with Labor Market

The subsistence farm does NOT participate in the auction. Instead:

```
Minimum reservation wage = 20 (subsistence wage)

Any worker can always choose subsistence farming for 20.
Therefore, no facility can hire below 20.
```

Workers only take subsistence jobs when no facility offers more than 20 for their slot.

## Market Clearing: Auction Model

Facilities bid for workers based on their MVP. Workers go to highest bidders.

### Mechanism

```
1. Each facility calculates MVP for each worker slot
2. Collect all slots across all facilities, sorted by MVP (descending)
3. Collect all workers, sorted by reservation wage (ascending)
4. Match highest-MVP slot to lowest-reservation worker
5. Wage paid = reservation + (MVP - reservation) * 0.5  (split surplus evenly)
6. Continue until MVP < reservation (no profitable matches remain)
7. Unmatched workers go to subsistence farming at wage 20
```

### Surplus Split

Workers and facilities split the surplus 50/50:

```
wage_paid = reservation + (MVP - reservation) * 0.5
```

This is simple and fair. The curves already capture market tightness dynamics — when labor is scarce, reservations are higher, so wages rise naturally.

### Example

```
Settlement has:
  - 100 workers, reservations ranging from 20 to 40 based on wealth
  - Bakery with 20 slots, MVP = 60 each
  - Farm with 50 slots, MVP = 35 each
  - Subsistence farm with unlimited slots, wage = 20

Auction proceeds:
  1. Bakery slots (MVP=60) hire workers with reservations 20-28
     Wages paid: 40-44 (midpoint of 60 and reservation)
  2. Farm slots (MVP=35) hire workers with reservations 28-35
     Wages paid: 31-35
  3. Workers with reservation > 35 go to subsistence at wage 20
     (They demanded more than any facility would pay)
```

### Advantages

- Profitable facilities naturally outcompete unprofitable ones
- Wage differentiation emerges (bakery pays more than farm)
- No explicit "profit gating" logic — market handles it
- Subsistence provides floor, prevents unemployment crisis

## Hysteresis and Smoothing

To prevent oscillation, wages and labor participation should change gradually.

### Wage Stickiness

```
target_wage = calculated clearing wage from curves
wage_change = (target_wage - current_wage) * adjustment_rate

If |wage_change| < threshold:
  actual_change = 0  (no change for small movements)
Else:
  actual_change = clamp(wage_change, -max_change, +max_change)

new_wage = current_wage + actual_change
```

Suggested parameters:
- `adjustment_rate`: 0.2 (20% of gap per tick)
- `threshold`: 0.5 (ignore changes < 0.5)
- `max_change`: 2.0 (cap large swings)

### Participation Smoothing

Labor supply response to wealth changes should also be gradual:

```
target_wealth_factor = f(current_wealth / target_wealth)
wealth_factor_change = (target - current) * adjustment_rate

Apply same threshold and clamping as wages
```

## Wage Bounds and Numeric Anchoring

The subsistence wage anchors the entire economy:

```
SUBSISTENCE_WAGE = 20  (the numeraire)

min_wage = 20  (subsistence farming always available)
max_wage = 200  (even extreme scarcity has limits)
```

**Wage scale (rough guide):**
```
20     = poverty line (subsistence farming)
30-40  = working class (basic facility job)
50-70  = skilled/in-demand labor
80-120 = labor shortage / boom times
150+   = extreme scarcity
```

**Price anchoring (emergent):**

Provision prices soft-anchor around 20 because:
- Subsistence workers earn 20
- They need ~1 provision to survive
- If price >> 20: workers can't afford food → population declines → demand falls → price falls
- If price << 20: workers have surplus → population grows → demand rises → price rises

Other prices float relative to provisions based on production chains and demand.

## Money Flows

```
For each employed worker:
  Facility owner treasury  →  Population wealth
  Amount: wage_paid

Aggregate:
  total_wages = Σ (workers_at_facility * wage) for all facilities
  org.treasury -= wage_bill_for_org
  population.wealth += total_wages_at_settlement
```

Note: Wages flow directly to wealth (the stock), not to a separate "income" variable.

## Open Questions

1. ~~**Surplus split in auction model**~~ — **Decided: 50/50 split.** Simple, fair, curves handle market dynamics.

2. ~~**Facility wage differentiation**~~ — **Decided: Auction model with differentiation.** Facilities pay based on their MVP, creating natural wage spread.

3. ~~**Unemployment effects**~~ — **Decided: Subsistence farming backstop.** No true unemployment; workers fall back to subsistence at wage 20 with diminishing returns on output.

4. **Skill differentiation** — Deferred. Currently all workers are fungible. Future: skilled vs unskilled labor pools?

5. **Migration** — Deferred. Should workers move between settlements seeking better wages? Adds equilibrating mechanism but also complexity.

6. **Information lag** — Deferred. Do workers know facility MVPs? Do facilities know worker reservations? Or is there search/matching friction?
