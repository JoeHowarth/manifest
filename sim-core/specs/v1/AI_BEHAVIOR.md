# AI Behavior Specification

## Overview

This document describes the decision points available to org AIs (and players). These are strategic choices made on top of the core economic mechanics.

**Key principles:**
- **Decision points are explicit**: Clear enumeration of what AIs can control
- **Sensible defaults**: Default heuristics mirror population/market curves, but AIs can do better
- **Information advantage**: Smarter AIs exploit price differentials, anticipate shortages
- **No construction for v1**: Cannot create/buy/sell facilities or ships

## Decision Points

### 1. Facility Labor Priority

**What**: Order in which org's facilities bid for workers at a settlement.

**When**: Before labor auction each tick.

**Options**:
- Priority list: `[FacilityId]` — facilities bid in this order
- Default: Sort by output value (provisions first, then inputs to provisions, etc.)

**Why it matters**: In a tight labor market, high-priority facilities get workers first. Prioritize bakeries to ensure provisions, or prioritize high-margin facilities for profit.

```
Decision: set_facility_priorities(settlement_id, facility_ids: Vec<FacilityId>)
```

### 2. Input Purchasing

**What**: How aggressively to buy inputs for facilities.

**When**: After production, during inventory management phase.

**Parameters**:
- `target_input_ticks`: How many ticks of inputs to maintain (default: 5)
- `buy_urgency_curve`: Function from (shortfall_ratio, treasury_ratio) → urgency (0-1)
- `max_price_curve`: Function from (shortfall_ratio, treasury_ratio) → max price multiplier

**Default heuristic** (mirrors population buying):
```
shortfall_ratio = shortfall / target
treasury_ratio = treasury / target_treasury

// Buy urgency
if treasury_ratio > 1.5:
    buy_urgency = shortfall_ratio * 1.0
elif treasury_ratio > 0.8:
    buy_urgency = shortfall_ratio * 0.7
elif treasury_ratio > 0.3:
    buy_urgency = shortfall_ratio * 0.4
else:
    buy_urgency = shortfall_ratio * 0.1

// Max price (as multiplier of current market price)
if shortfall_ratio > 0.8 and treasury_ratio > 1.0:
    max_price_mult = 1.5  // Desperate and can afford it
elif shortfall_ratio > 0.5:
    max_price_mult = 1.2  // Moderately urgent
else:
    max_price_mult = 1.0  // Only buy at market price or below
```

**Smart AI alternatives**:
- Buy ahead of anticipated shortages
- Buy when prices are low, even if buffer is full
- Refuse to buy at inflated prices, let production pause

```
Decision: set_input_buying_params(facility_id, target_ticks, urgency_curve, max_price_curve)
```

### 3. Output Selling

**What**: How aggressively to sell produced goods.

**When**: After production, during inventory management phase.

**Parameters**:
- `target_output_ticks`: How many ticks of output to hold (default: 2)
- `sell_urgency_curve`: Function from (surplus_ratio, treasury_ratio) → urgency (0-1)
- `min_price_curve`: Function from (surplus_ratio, treasury_ratio) → min price multiplier

**Default heuristic**:
```
surplus_ratio = (current - target) / target
treasury_ratio = treasury / target_treasury

// Sell urgency
if treasury_ratio < 0.3:
    sell_urgency = 1.0  // Liquidate, need cash
elif treasury_ratio < 0.8:
    sell_urgency = 0.8
elif surplus_ratio > 1.0:
    sell_urgency = 0.7  // Clear excess inventory
else:
    sell_urgency = 0.4  // Sell some, hold some

// Min price (as multiplier of current market price)
if treasury_ratio < 0.3:
    min_price_mult = 0.7  // Desperate, accept low prices
elif surplus_ratio > 2.0:
    min_price_mult = 0.8  // Need to clear inventory
else:
    min_price_mult = 0.95  // Hold out for fair price
```

**Smart AI alternatives**:
- Hold inventory when prices are low, sell when high
- Dump goods to crash a competitor's market
- Coordinate sales across settlements for arbitrage

```
Decision: set_output_selling_params(facility_id, target_ticks, urgency_curve, min_price_curve)
```

### 4. Ship Routing

**What**: Where ships go and what they carry.

**When**: When ship is in port and available.

**Options**:

#### A. Direct Control (per-ship orders)

Issue specific orders to individual ships:

```
Decision: send_ship(ship_id, destination, cargo_to_load: Vec<(Good, f32)>)
```

Ship will:
1. Load specified cargo from org warehouse at current port
2. Travel to destination
3. Unload cargo to org warehouse at destination
4. Await further orders

#### B. Recurring Trade Routes

Set up standing orders that repeat automatically:

```
Decision: set_trade_route(ship_id, route: TradeRoute)

TradeRoute {
    stops: Vec<RouteStop>,
    repeat: bool,
}

RouteStop {
    settlement_id: SettlementId,
    load: Vec<(Good, f32)>,      // Load up to this much
    unload: Vec<Good>,           // Unload all of these goods
    wait_for_cargo: bool,        // Wait until cargo available?
}
```

Example — Flour shuttle between Hartwen and Osmouth:
```
TradeRoute {
    stops: [
        RouteStop {
            settlement: Hartwen,
            load: [(Flour, 50)],
            unload: [Provisions],
            wait_for_cargo: false
        },
        RouteStop {
            settlement: Osmouth,
            load: [(Provisions, 30)],
            unload: [Flour],
            wait_for_cargo: false
        },
    ],
    repeat: true,
}
```

#### C. Opportunistic Trading (Default AI)

Let AI decide based on price differentials:

```
Decision: set_ship_mode(ship_id, ShipMode::Opportunistic)
```

Default heuristic:
1. Find all goods in org warehouse at current port
2. For each reachable settlement, calculate price differential
3. Score opportunities: `(dest_price - local_price) * available_qty`
4. Pick best opportunity, load cargo, send ship
5. If no profitable opportunity, wait or reposition to better port

**Smart AI alternatives**:
- Anticipate future prices based on production/consumption patterns
- Coordinate multiple ships to avoid competing with self
- Position ships ahead of seasonal demand
- Triangular trade routes (A→B→C→A)

### 5. Cargo Buy/Sell at Port

**What**: Explicit buy/sell orders for goods at current ship location.

**When**: Ship is in port.

**Why**: Separate from facility inventory management. Ships can engage in pure trading — buy low at one port, sell high at another — without owning local facilities.

```
Decision: ship_buy(ship_id, good, quantity, max_price)
Decision: ship_sell(ship_id, good, quantity, min_price)
```

The ship's cargo acts as mobile inventory. Org pays/receives from treasury.

**Default AI**: Buy goods where cheap, sell where expensive (classic arbitrage).

## Information Available to AI

AIs make decisions based on available information. This can be tuned for difficulty/realism.

### Full Information (v1 default)

All orgs see:
- All market prices at all settlements
- All inventory levels (own warehouses)
- Ship positions and cargo
- Facility status

### Limited Information (future)

Orgs only know:
- Prices at settlements where they have presence (facility or recent ship visit)
- Price information ages (last known price, N ticks ago)
- Rumors/estimates for unknown markets

## Default AI Summary

The default AI uses simple heuristics that mirror population behavior:

| Decision | Default Behavior |
|----------|------------------|
| Facility priority | Provisions-producing facilities first |
| Input buying | Buy more when buffer low AND treasury healthy |
| Output selling | Sell more when buffer high OR treasury low |
| Ship routing | Opportunistic arbitrage based on price differentials |
| Cargo trading | Buy low, sell high |

These defaults create a functional economy. Smarter AIs (or players) can outperform by:
- Anticipating price movements
- Coordinating across settlements
- Strategic inventory holding
- Optimal ship positioning

## What AIs Cannot Do (v1)

The following are **not** available as AI decisions in v1:

- **Build facilities**: No construction system yet
- **Buy/sell facilities**: No facility market
- **Build ships**: No shipyard production chain active
- **Buy/sell ships**: No ship market
- **Hire/fire specific workers**: Labor market handles allocation
- **Set wages directly**: Wages emerge from auction
- **Set prices directly**: Prices emerge from market clearing

## Open Questions

All deferred — will iterate on AI policies through implementation and testing:

1. **Route optimization** — Pathfinding helpers vs manual enumeration
2. **Multi-ship coordination** — Preventing self-competition
3. **Risk assessment** — Factoring route risk into decisions
4. **Contracts** — Recurring purchase agreements between orgs
5. **Information trading** — Selling/sharing price information
