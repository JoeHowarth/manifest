# Facilities, Orgs & Production Specification

## Overview

This document describes how production works: facilities that transform inputs into outputs, organizations that own and operate them, and the money flows that connect production to the broader economy.

**Key principles:**
- **Input-gated production**: No inputs = no production = no labor demand
- **Org inventory targets**: Orgs manage input/output buffers like populations manage stockpiles
- **MVP from actual profitability**: Labor demand based on real prices and available inputs
- **Settlement as special org**: Owns subsistence farm, anchors the economy
- **Closed money loop**: Wages → population → purchases → org revenue → wages

## Organizations (Orgs)

### Structure

```
Org {
    name: String,
    treasury: f32,           // Money on hand

    // Inventory targets (per settlement where org operates)
    input_target_ticks: u32,   // Target buffer of inputs (e.g., 5 ticks)
    output_target_ticks: u32,  // Target buffer of outputs (e.g., 2 ticks)
}
```

### Org Types

**Regular Orgs** (player or AI-controlled):
- Own facilities and ships
- Manage treasury and inventory
- Make production and trade decisions
- Goal: profit and growth

**Settlement Org** (special):
- One per settlement
- Owns the subsistence farm
- Treasury receives subsistence farm revenue
- Pays subsistence farm wages (fixed at 20)
- May receive taxes in future

### Treasury Dynamics

Money flows for regular orgs:
```
INFLOWS:
  + Revenue from selling goods
  + (Future: dividends, loans)

OUTFLOWS:
  - Wages to workers
  - Purchases of inputs from market
  - (Future: maintenance, taxes)
```

Treasury can grow (profitable operation) or shrink (unprofitable or ramping up).

## Facilities

### Structure

```
Facility {
    kind: FacilityType,
    owner: OrgId,            // Which org owns this
    location: SettlementId,  // Where it's located
    optimal_workforce: u32,  // Workers for peak efficiency
    current_workforce: u32,  // Workers actually employed this tick
    efficiency: f32,         // Output multiplier (tools, etc.)
}
```

### Facility Types

**Primary (extraction)**:
| Type | Output | Required Resource |
|------|--------|-------------------|
| Farm | Grain | Fertile Land |
| Fishery | Fish | Fishery |
| Lumber Camp | Lumber | Forest |
| Mine | Ore | Ore Deposit |
| Pasture | Wool | Pastureland |

**Processing (transformation)**:
| Type | Inputs | Output |
|------|--------|--------|
| Mill | Grain | Flour |
| Foundry | Ore | Iron |
| Weaver | Wool | Cloth |

**Finished goods**:
| Type | Inputs | Output |
|------|--------|--------|
| Bakery | Flour, Fish | Provisions |
| Toolsmith | Lumber, Iron | Tools |

**Capital goods**:
| Type | Inputs | Output |
|------|--------|--------|
| Shipyard | Lumber, Iron, Cloth | Ships |

**Special**:
| Type | Inputs | Output | Notes |
|------|--------|--------|-------|
| Subsistence Farm | (none) | Provisions | Settlement-owned, wage=20, diminishing returns |

### Recipes

Each facility type has a recipe:

```
Recipe {
    inputs: Vec<(Good, f32)>,  // (good, amount per unit output)
    output: Good,
    base_output: f32,          // Units per tick at full efficiency
    optimal_workforce: u32,    // Workers for full production
}
```

Example (Bakery):
```
inputs: [(Flour, 0.8), (Fish, 0.3)]  // Per unit of provisions
output: Provisions
base_output: 40.0
optimal_workforce: 100
```

## Production Cycle

Each tick, production follows these phases:

### Phase 1: Calculate Capacity

For each facility, determine production capacity based on inputs:

```
For each input (good, ratio) in recipe:
    required = base_output * ratio
    available = owner_warehouse.get(good)
    input_fraction = min(1.0, available / required)

input_efficiency = min(input_fraction for all inputs)
max_production = base_output * input_efficiency
```

**Key rule**: If inputs are insufficient, production scales down proportionally.

### Phase 2: Calculate Labor Demand (MVP)

Facilities only demand labor for production they can actually do:

```
// Scale optimal workforce by input availability
effective_optimal = optimal_workforce * input_efficiency

// Calculate MVP for each worker slot
output_price = local_market.price(recipe.output)
input_cost_per_unit = sum(recipe.inputs.map(|(g, r)| local_market.price(g) * r))

marginal_output_per_worker = base_output / optimal_workforce
value_per_worker = marginal_output_per_worker * output_price
cost_per_worker = marginal_output_per_worker * input_cost_per_unit

MVP = value_per_worker - cost_per_worker
```

**If MVP ≤ 0**: Facility doesn't bid for workers (unprofitable)
**If input_efficiency = 0**: Facility doesn't bid (no inputs)

### Phase 3: Labor Auction

See LABOR_MARKET.md. Facilities bid based on MVP, workers allocated to highest bidders.

### Phase 4: Execute Production

After labor is allocated:

```
workforce_efficiency = current_workforce / optimal_workforce
// Cap at 1.0, or allow slight over-staffing with diminishing returns

actual_efficiency = min(input_efficiency, workforce_efficiency) * facility.efficiency
actual_output = base_output * actual_efficiency

// Consume inputs
For each (good, ratio) in recipe.inputs:
    consumed = actual_output * ratio
    owner_warehouse.remove(good, consumed)

// Produce output
owner_warehouse.add(recipe.output, actual_output)
```

### Phase 5: Inventory Management

After production, orgs manage their inventories (see Org Inventory Management below).

## Org Inventory Management

Orgs maintain target inventory levels, similar to how populations maintain stockpiles.

> **Note**: The specific buying/selling heuristics are AI decisions. See `AI_BEHAVIOR.md` for full details on decision points, curves, and smart alternatives. This section describes the mechanical framework.

### Target Levels

```
For each facility owned by org at settlement:
    For each input good:
        target_input = recipe.input_rate * input_target_ticks  // e.g., 5 ticks

    target_output = recipe.base_output * output_target_ticks  // e.g., 2 ticks
```

### Buying Behavior (Inputs)

The org decides how much to buy based on:
- **Shortfall**: How far below target inventory?
- **Treasury health**: Can we afford to restock?
- **Max price**: What's the most we'll pay?

```
buy_amount = f(shortfall, treasury_ratio, urgency_curve)
max_price = g(shortfall, treasury_ratio, price_curve)

actual_bought = market.buy(good, buy_amount, max_price)
treasury -= actual_bought * price
```

Default heuristics mirror population buying curves (buy more when buffer low and wealthy). Smart AIs can anticipate shortages or refuse inflated prices.

### Selling Behavior (Outputs)

The org decides how much to sell based on:
- **Surplus**: How far above target inventory?
- **Treasury health**: Do we need cash?
- **Min price**: What's the least we'll accept?

```
sell_amount = f(surplus, treasury_ratio, urgency_curve)
min_price = g(surplus, treasury_ratio, price_curve)

actual_sold = market.sell(good, sell_amount, min_price)
treasury += actual_sold * price
```

Default heuristics sell more when treasury is low or inventory is high. Smart AIs can hold for better prices or dump to crash markets.

### Target Treasury

Orgs need operating capital for wages and input purchases:

```
expected_wage_bill = sum(facility.optimal_workforce * expected_wage for facilities)
expected_input_cost = sum(facility input needs * input prices)

target_treasury = (expected_wage_bill + expected_input_cost) * buffer_ticks  // e.g., 5 ticks
```

## Subsistence Farm (Special Facility)

### Properties

```
Subsistence Farm:
    owner: Settlement (special org)
    location: Settlement
    optimal_workforce: 0.5 * settlement.population  // High capacity
    wage: 20  // FIXED, does not participate in auction
    output: Provisions
    inputs: (none)
```

### Diminishing Returns

Unlike regular facilities, subsistence farms have steep diminishing returns:

```
// Output per worker based on position in queue
worker_slot 1-100:     output_per_worker = 1.2
worker_slot 101-300:   output_per_worker = 0.8
worker_slot 301-500:   output_per_worker = 0.5
worker_slot 500+:      output_per_worker = 0.3
```

Alternative formula:
```
output_per_worker(n) = 1.5 * (1 / (1 + n/100))

// First worker: 1.5 / 1.01 ≈ 1.49
// Worker 100:   1.5 / 2.0  = 0.75
// Worker 300:   1.5 / 4.0  = 0.375
```

### Integration with Economy

```
1. Workers not hired by regular facilities fall to subsistence
2. Subsistence farm pays wage 20 from settlement treasury
3. Subsistence farm produces provisions (diminishing per worker)
4. Provisions sold to market at market price
5. Revenue goes to settlement treasury

Settlement treasury balance:
    + provision_sales
    - wages_paid (workers * 20)

If sales < wages: settlement depletes treasury (unsustainable)
If sales > wages: settlement accumulates (small profit)
```

The equilibrium is typically near break-even: provisions price ≈ 20 per worker's consumption.

## Money Circulation

### The Closed Loop

```
┌──────────────────────────────────────────────────────────────────┐
│                                                                  │
│   ┌─────────┐    wages    ┌────────────┐   purchases   ┌─────┐  │
│   │  ORGS   │────────────►│ POPULATION │──────────────►│MARKET│ │
│   │         │◄────────────│            │◄──────────────│     │  │
│   └────┬────┘   (sells)   └────────────┘    (goods)    └──┬──┘  │
│        │                                                   │     │
│        │ buys inputs                          sells outputs│     │
│        └───────────────────────────────────────────────────┘     │
│                                                                  │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │                    SETTLEMENT ORG                        │   │
│   │  Subsistence Farm: pays wages → sells provisions         │   │
│   └─────────────────────────────────────────────────────────┘   │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### Balance Conditions

**Orgs accumulate when**: Revenue > (Wages + Input purchases)
- This is profit
- Money leaves population circulation
- Population has less purchasing power
- Demand falls → prices fall → revenue falls
- Self-correcting toward balance

**Orgs deplete when**: Revenue < (Wages + Input purchases)
- Ramping up production, or
- Prices crashed, or
- Inefficient operation
- Eventually org can't pay wages → workers leave → production stops

**Population accumulates when**: Wages > Spending
- Saving for buffer
- Prices low relative to income
- Eventually: work less (higher reservations) OR spend more (luxury goods)

**Population depletes when**: Wages < Spending
- Drawing down savings
- Prices high relative to income
- Eventually: work more (lower reservations) OR spend less (rationing)

## Tick Sequence (Production View)

```
1. LABOR MARKET
   - Facilities calculate MVP based on inputs and prices
   - Auction allocates workers
   - Wages paid: Org treasury → Population wealth

2. PRODUCTION
   - Facilities consume inputs from warehouse
   - Facilities produce outputs to warehouse

3. ORG INVENTORY MANAGEMENT
   - Calculate shortfalls and surpluses
   - Buy inputs from market (treasury → other orgs)
   - Sell outputs to market (goods → market availability)

4. GOODS MARKET
   - Population buys from market (wealth → org treasury)
   - Prices adjust based on supply/demand

5. CONSUMPTION
   - Population consumes from stockpile
   - Population effects (growth/decline)
```

## Open Questions

1. ~~**Input gating**~~ — **Decided**: No inputs = no production = MVP of 0. Partial inputs = scaled production.

2. ~~**Org inventory targets**~~ — **Decided**: 5 ticks of inputs, 2 ticks of outputs. Buy/sell based on treasury health.

3. ~~**Settlement org**~~ — **Decided**: Special org that owns subsistence farm, has treasury.

4. **Facility maintenance** — Deferred. No explicit maintenance costs for v1. Watch for money getting stuck; add maintenance as labor cost if needed.

5. **Facility construction** — How do new facilities get built? Cost in goods and money? Construction time? Deferred for v1.

6. **Facility degradation** — Do unmaintained facilities degrade? Lose efficiency? Get destroyed? Deferred.

7. **Max prices for buying** — When orgs buy inputs, should there be a max price they'll pay? Or just buy whatever's available?

8. **Inter-org trade** — Currently through market only. Direct contracts between orgs? Deferred.
