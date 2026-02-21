# Population Trap Analysis

## Setup

worst_case scenario: price=2.0, pop_stock=2.0, merchant_stock=1.0, 100 pops, 2 facilities (cap 50 each), production_rate=1.05. External anchor and subsistence reservation enabled.

## What happens

Population crashes from 100 to ~37 in the first 20 ticks. Then locks there for the remaining 280 ticks. Only a handful of births across 300 ticks. Pop stock slowly declines from 2.0 toward 1.0 — heading for a second collapse.

## The feedback loops (corrected analysis)

The initial analysis blamed the merchant sell ladder for not releasing surplus aggressively enough. **That was wrong.** The merchant offers ~90 units/tick — plenty. The problem is demand-side: pops can't afford to buy more than ~1 unit each.

### Loop 1: Wage-price structural identity

The facility bid system computes MVP = marginal_output × grain_price = 1.05 × P. Wages converge toward MVP, so wage ≈ 1.05 × price. But market friction (EMA smoothing, clearing dynamics) eats ~4% of that margin. Observed: **wage/price = 1.006**.

Each pop can afford exactly 1.006 units of grain per tick. Subsistence requirement is 1.0. The margin for surplus consumption is 0.6% — effectively nothing.

This is a **macroeconomic identity** in a single-good, single-employer economy: all wages flow back as revenue, so P × Q = W × N, and with Q ≈ N (each pop buys ~1 unit), P ≈ W.

### Loop 2: Consumption cap blocks surplus eating

Even the tiny 0.6% surplus doesn't reach food_satisfaction because:

- Pop stock ≈ 1.7 units (declining from initial 2.0)
- target = desired_ema × 5.0 ≈ 5.0
- stock/target ≈ 0.34 — **well below the 0.6 surplus_release threshold**
- surplus_release_factor = 0.000 → capped actual consumption = exactly 1.0
- food_satisfaction = 1.001 → growth_probability = 0.008%

The consumption cap was designed to prevent buffer depletion during good ticks. But it has the side effect of **completely blocking surplus eating when stock is below 60% of target** — which is always, in this scenario.

### Loop 3: EMA self-confirmation

Both pop bids and merchant asks are computed as fractions of price_EMA (sweep over 0.6-1.4 × EMA). Clearing happens right at the EMA (clearing/EMA = 0.999 ± 0.023). The EMA is self-confirming — there's no mechanism to push it away from the trapped equilibrium.

### Loop 4: External drain removes the surplus

The external anchor exports ~1.75 grain/tick — **nearly the entire production surplus** (37 × 0.05 = 1.85). This prevents even slow accumulation in the merchant stockpile that might eventually create downward price pressure.

## Key data (from instrumented feedback loop investigation)

### Trapped-phase averages (t=100-300, 37 pops)

| Metric | Value |
|--------|-------|
| wage | 0.4180 |
| clearing price | 0.4157 |
| wage/price | 1.006 |
| pop stock | 1.68 (declining) |
| actual consumed per pop | 1.001 |
| fill per pop | 0.994 |
| surplus_release_factor | 0.000 |
| merchant stock | 145 → 94 (declining) |
| external exports | 1.75 grain/tick |
| production | 38.86 grain/tick |
| consumption | 37.05 grain/tick |
| net residual | 0.06 grain/tick |

### Tick-by-tick clearing vs EMA

| tick | clearing | EMA | ratio |
|------|----------|-----|-------|
| 20 | 0.681 | 0.685 | 0.994 |
| 50 | 0.431 | 0.429 | 1.004 |
| 100 | 0.411 | 0.409 | 1.005 |
| 200 | 0.400 | 0.398 | 1.006 |
| 299 | 0.428 | 0.427 | 1.004 |

### Stock depletion trajectory

| tick | avg pop stock | stock/target | surplus_release | consumption cap |
|------|--------------|--------------|-----------------|-----------------|
| 20 | 3.30 | 0.62 | 0.003 | 1.08 |
| 50 | 3.17 | 0.61 | 0.002 | 1.04 |
| 100 | 2.46 | 0.49 | 0.000 | 1.00 |
| 200 | 1.55 | 0.31 | 0.000 | 1.00 |
| 299 | 1.00 | 0.20 | 0.000 | 1.00 |

Stock is declining at ~0.007/pop/tick because fill_per_pop (0.994) < actual_consumed (1.001). At this rate, pops hit zero stock around tick 350-400, triggering a second mortality wave.

## Why this differs from a real economy

In a real pre-industrial economy, a surplus of 5% above subsistence DOES drive population growth (slowly). The mechanisms that make it work:

1. **Price competition** between multiple merchants → drives prices below wage → real purchasing power > 1.0
2. **Wage competition** between multiple employers → bids up wages → purchasing power increases
3. **Investment** — merchant surplus reinvested as capital → more capacity → more jobs → higher wages
4. **Multiple goods** — breaks the P ≈ W identity because workers can produce non-food goods worth more than food
5. **Stockpiling** — real people eat more when food is cheap, building body reserves (no consumption cap)

## Root cause hierarchy

1. **Structural**: Single-good, single-employer economy creates the identity wage ≈ price × 1.05. Maximum real surplus is 5%, and market friction eats most of it.
2. **Proximate**: Consumption cap (`surplus_release_factor = 0` when stock/target < 0.6) clips the remaining 0.6% surplus to zero, making food_satisfaction = 1.000.
3. **Secondary**: External anchor drains 1.75 grain/tick (= the entire production surplus), preventing any accumulation that might eventually break the price equilibrium.
4. **Amplifying**: EMA self-confirmation means there's no price discovery mechanism to escape the equilibrium.

## Implications for the simulation

This trap is **not a bug in any single system** — it's an emergent property of multiple systems interacting. The sell ladder, consumption cap, budget rules, and MVP-based wages are each reasonable in isolation. Together they create an inescapable equilibrium.

Options for addressing (in order of structural depth):
- **Multiple goods/employers**: The real fix — breaks the wage-price identity. But requires game content, not sim tuning.
- **Investment mechanism**: Let merchants reinvest surplus as facility upgrades or wage bonuses. Creates a return channel for accumulated capital.
- **Soften consumption cap at low populations**: Allow surplus eating even at low stock/target ratios when food_satisfaction is near 1.0 and growth_probability is near zero.
- **Reduce external drain in stressed economies**: Scale external anchor depth by economic health, not just pop count.
- **Widen the growth probability window**: Currently 0% → 2% across food_sat 1.0-1.25. A wider or steeper curve would amplify even tiny surpluses.
