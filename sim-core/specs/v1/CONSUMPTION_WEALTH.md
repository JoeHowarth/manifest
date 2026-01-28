# Consumption & Wealth Specification

## Overview

This document describes how settlement populations manage their economic lives: earning income, accumulating wealth, purchasing goods, consuming from stockpiles, and how these dynamics affect population growth and decline.

**Key principles:**
- **Two buffers**: Wealth (money savings) and Stockpile (goods reserves)
- **Wealth is accounting**: `d_wealth = income - spending`, nothing magical
- **Non-linear rationing**: Consumption adjusts to stockpile level, but not proportionally
- **Spending from wealth**: Budget is based on wealth (the stock), not income (the flow)
- **Population feedback**: Consumption satisfaction drives growth/decline

## The Two Buffers

### Wealth Buffer (Money Savings)

Population wealth represents accumulated savings — the stock of money available for purchases.

```
wealth: f32  // Total savings for the population
target_wealth: f32  // "Comfortable" level of savings
wealth_ratio = wealth / target_wealth
```

**Target wealth** represents what the population considers "comfortable" — enough savings to weather short-term disruptions. When below target, people feel insecure; when above, they feel prosperous.

Suggested: `target_wealth = population * 50` (50 per capita at anchor wage of 20)

### Goods Stockpile (Physical Reserves)

Population stockpiles represent household reserves of goods — food in the pantry, clothes in the closet.

```
stockpile_provisions: f32  // Provisions on hand
stockpile_cloth: f32  // Cloth on hand

target_provisions = population * consumption_rate * buffer_ticks
target_cloth = population * cloth_rate * buffer_ticks
```

**Buffer ticks** is how many ticks of consumption people want to keep on hand. Larger buffer = more stability, but requires more purchasing.

Suggested: `buffer_ticks = 5` (one work-week of reserves)

## The Tick Cycle

Each tick, population goes through these phases in order:

```
1. CONSUME from stockpile
2. EARN income (wages deposited to wealth)
3. DECIDE spending budget
4. BUY from market (spend from wealth, add to stockpile)
5. POPULATION effects (growth/decline based on consumption)
```

## Phase 1: Consumption

Population consumes goods from their stockpile each tick. The amount consumed depends on stockpile level — people ration when reserves are low.

### Provisions (Essential)

```
base_need = population / 100  // ~1 provision per 100 people per tick

stockpile_ratio = stockpile_provisions / target_provisions
consumption_mult = consumption_curve(stockpile_ratio)

actual_consumption = base_need * consumption_mult
stockpile_provisions -= actual_consumption
```

### The Consumption Curve (Rationing)

The curve should be **concave** — ration when low, but not proportionally:

```
stockpile_ratio → consumption_mult

0.0  →  0.0   (nothing to eat)
0.2  →  0.5   (severe rationing, but not 20%)
0.4  →  0.7   (moderate rationing)
0.6  →  0.85  (mild rationing)
1.0  →  1.0   (full consumption)
1.5  →  1.05  (slight indulgence)
2.0+ →  1.1   (cap on over-consumption)
```

**Formula option 1** (power curve):
```
consumption_mult = min(1.1, stockpile_ratio^0.5)
```

**Formula option 2** (smoothstep with cap):
```
if stockpile_ratio <= 0:
    consumption_mult = 0
elif stockpile_ratio < 1:
    consumption_mult = smoothstep(stockpile_ratio)  // ease-in curve
else:
    consumption_mult = min(1.1, 1.0 + (stockpile_ratio - 1) * 0.1)
```

### Cloth (Non-Essential)

Cloth consumption follows similar logic but with different consequences:

```
base_want = population * wealth_ratio / 500  // Wealthy want more cloth
actual_cloth_consumption = base_want * consumption_curve(stockpile_cloth / target_cloth)
stockpile_cloth -= actual_cloth_consumption
```

Cloth shortages don't kill people, but affect wealth accumulation (see Population Effects).

## Phase 2: Income

Wages earned this tick are deposited directly into wealth:

```
wealth += wages_earned
```

Note: Wages come from the labor market (see LABOR_MARKET.md). This includes both facility wages and subsistence farming wages.

There is no separate "income" variable — wages flow directly to the wealth stock.

## Phase 3: Spending Decision

The population decides how much to spend based on:
1. **Stockpile urgency**: How far below target are the stockpiles?
2. **Wealth comfort**: How much wealth do we have relative to target?
3. **Prices**: What do goods cost?

### Spending Budget

```
provisions_urgency = max(0, target_provisions - stockpile_provisions) / target_provisions
cloth_urgency = max(0, target_cloth - stockpile_cloth) / target_cloth

wealth_ratio = wealth / target_wealth
```

**Spending willingness** based on wealth:

```
if wealth_ratio > 1.5:
    // Wealthy: spend freely to restock
    spend_willingness = 0.4
elif wealth_ratio > 0.8:
    // Comfortable: normal spending
    spend_willingness = 0.25
elif wealth_ratio > 0.3:
    // Tight: conservative spending
    spend_willingness = 0.15
else:
    // Poor: minimal spending, preserve what little wealth remains
    spend_willingness = 0.08
```

**Budget allocation**:

```
total_budget = wealth * spend_willingness

// Provisions get priority based on urgency
provisions_priority = 0.7 + provisions_urgency * 0.2  // 0.7 to 0.9
cloth_priority = 1.0 - provisions_priority  // 0.1 to 0.3

provisions_budget = total_budget * provisions_priority
cloth_budget = total_budget * cloth_priority
```

When provisions are critically low, they get up to 90% of budget. Cloth gets the remainder.

### Purchase Limits

Actual purchases are limited by budget, market supply, and actual need:

```
provisions_shortfall = max(0, target_provisions - stockpile_provisions)
provisions_affordable = provisions_budget / price_provisions
provisions_available = market_supply_provisions

provisions_to_buy = min(provisions_shortfall, provisions_affordable, provisions_available)
```

Similar for cloth.

## Phase 4: Market Purchase

Execute the purchases:

```
// Buy provisions
actual_provisions_bought = execute_market_buy(Good::Provisions, provisions_to_buy)
provisions_cost = actual_provisions_bought * price_provisions
stockpile_provisions += actual_provisions_bought

// Buy cloth
actual_cloth_bought = execute_market_buy(Good::Cloth, cloth_to_buy)
cloth_cost = actual_cloth_bought * price_cloth
stockpile_cloth += actual_cloth_bought

// Deduct from wealth
total_spending = provisions_cost + cloth_cost
wealth -= total_spending
```

Note: `execute_market_buy` handles the market clearing mechanics, potentially returning less than requested if supply is insufficient.

## Phase 5: Population Effects

Population growth/decline is driven by consumption satisfaction.

### Provision Satisfaction

```
provision_satisfaction = actual_consumption / base_need

// Where actual_consumption is from Phase 1
// This reflects both stockpile availability AND rationing behavior
```

### Population Change

```
if provision_satisfaction < 0.3:
    // Starvation
    pop_change_rate = -0.03  // -3% per tick
elif provision_satisfaction < 0.5:
    // Severe shortage
    pop_change_rate = -0.015  // -1.5% per tick
elif provision_satisfaction < 0.8:
    // Mild shortage
    pop_change_rate = -0.005  // -0.5% per tick
elif provision_satisfaction < 1.0:
    // Getting by
    pop_change_rate = 0.0  // stable
elif provision_satisfaction >= 1.0 and wealth_ratio > 0.8:
    // Prosperous
    pop_change_rate = +0.002  // +0.2% per tick
else:
    // Fed but poor
    pop_change_rate = +0.001  // +0.1% per tick

population = population * (1 + pop_change_rate)
population = max(100, population)  // Minimum population floor
```

### Cloth Effects

Cloth doesn't affect survival but affects economic mobility:

```
cloth_satisfaction = actual_cloth_consumption / base_want

if cloth_satisfaction < 0.3:
    // Ragged, social penalty
    wealth_mult = 0.95  // Harder to accumulate wealth
elif cloth_satisfaction < 0.7:
    // Threadbare, minor penalty
    wealth_mult = 0.98
elif cloth_satisfaction > 1.2:
    // Well-dressed, social bonus
    wealth_mult = 1.02
else:
    wealth_mult = 1.0

// Apply at end of tick
wealth = wealth * wealth_mult
```

This is a soft effect — cloth poverty doesn't kill you, but it's a drag on prosperity.

## Wealth Dynamics

Wealth evolves purely through accounting:

```
d_wealth = wages_earned - spending + cloth_adjustment

wealth_next = wealth + d_wealth
wealth_next = max(0, wealth_next)  // Can't go negative
```

There are no magic wealth sources or sinks. If you earn more than you spend, wealth grows. If you spend more than you earn, wealth shrinks.

### The Poverty Trap

When wealth is low:
1. Spend willingness is low → can't restock adequately
2. Stockpile stays low → rationing continues
3. Consumption satisfaction low → population declines OR stagnates
4. Fewer workers → less production → less supply → high prices
5. High prices → even harder to restock

The escape routes:
- **External supply**: Trade brings goods, prices fall, restocking becomes affordable
- **Population decline**: Fewer mouths to feed, demand drops, balance restored (painful)
- **Productivity improvement**: Better facilities produce more per worker

### The Prosperity Spiral

When wealth is high:
1. Spend willingness is high → restock generously
2. Stockpile buffer maintained → no rationing
3. Full consumption → population grows (slowly)
4. More workers → more production (if facilities can employ them)
5. Wealth accumulates → labor supply shifts (higher reservations)
6. Wages rise → purchasing power rises → stability

The limiting factors:
- **Labor scarcity**: Population can't grow faster than birth rate
- **Facility capacity**: Production capped by facilities and inputs
- **Price increases**: High demand raises prices, limiting growth

## Interaction with Labor Market

The wealth level affects labor supply through reservation wages (see LABOR_MARKET.md):

```
wealth_ratio = wealth / target_wealth

if wealth_ratio > 1.5:
    // Wealthy: demand higher wages, fewer willing to work
    reservation_mult = 1.5
    labor_participation = 0.5
elif wealth_ratio > 1.0:
    // Comfortable
    reservation_mult = 1.2
    labor_participation = 0.55
elif wealth_ratio > 0.5:
    // Normal
    reservation_mult = 1.0
    labor_participation = 0.6
else:
    // Poor: accept lower wages, everyone works
    reservation_mult = 0.8
    labor_participation = 0.7

// Applied to reservation wage calculation
base_reservation = SUBSISTENCE_WAGE  // 20
adjusted_reservation = base_reservation * reservation_mult
```

This creates the feedback loop:
- Low wealth → work more, accept less → wages fall → purchasing power falls
- High wealth → work less, demand more → wages rise → purchasing power rises (but labor scarce)

## Summary of Flows

```
┌─────────────────────────────────────────────────────────────────┐
│                         POPULATION                               │
│                                                                  │
│  ┌─────────┐         ┌─────────────┐         ┌─────────────┐   │
│  │ WEALTH  │◄────────│   WAGES     │◄────────│   LABOR     │   │
│  │ (stock) │         │  (inflow)   │         │  MARKET     │   │
│  └────┬────┘         └─────────────┘         └─────────────┘   │
│       │                                                         │
│       │ spending                                                │
│       ▼                                                         │
│  ┌─────────┐         ┌─────────────┐         ┌─────────────┐   │
│  │ MARKET  │────────►│  STOCKPILE  │────────►│ CONSUMPTION │   │
│  │         │ buying  │  (buffer)   │  eating │             │   │
│  └─────────┘         └─────────────┘         └──────┬──────┘   │
│                                                      │          │
│                                              satisfaction       │
│                                                      ▼          │
│                                              ┌─────────────┐   │
│                                              │ POPULATION  │   │
│                                              │   CHANGE    │   │
│                                              └─────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## Open Questions

1. ~~**Stockpile storage limits**~~ — **Decided: Yes, but high limits.** Prevents infinite hoarding, creates warehouse gameplay later.

2. ~~**Spoilage**~~ — **Deferred.** No decay for now; revisit later if oversupply becomes a problem.

3. **Luxury goods** — Currently provisions (essential) and cloth (comfort). Cloth is a comfort good — baseline demand exists, but shortage doesn't kill, just drags on wealth. Future: true luxury goods (alcohol, spices, furniture) with demand that only appears when wealthy. Different demand curve:
   ```
   // True luxury demand (future)
   base_want = population * rate * max(0, wealth_ratio - 1.0)
   ```

4. ~~**Household vs aggregate**~~ — **Decided: Aggregate for now.** Simpler, sufficient for v1.

5. ~~**Information and expectations**~~ — **Decided: Reactive for now.** Population responds to current conditions, doesn't anticipate. AI-driven orgs can be smarter (speculate, stockpile ahead of shortages).
