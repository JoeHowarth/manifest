# AI Behavior Specification (v2)

## Overview

This document describes how different entities decide what bids and asks to submit to markets. All entities participate in the same auction mechanism; they differ in HOW they generate orders.

**Key principles:**
- **Same mechanism, different strategies** — everyone submits bids/asks, but logic differs
- **Population is reactive** — follows curves based on current state
- **Orgs are strategic** — can anticipate, hold inventory, optimize
- **Settlement org is hardcoded** — simple, predictable behavior

## Entity Behaviors

### Population (Hardcoded)

Population behavior is deterministic based on state. Not "AI" — just curves.

**Labor asks:**
```
Inputs: wealth, target_wealth, population
Output: Vec<Ask> for Labor

wealth_ratio = wealth / target_wealth
wealth_factor = match wealth_ratio:
    > 1.5  => 1.5   // Wealthy, demand premium
    > 1.0  => 1.2   // Comfortable
    > 0.5  => 1.0   // Normal
    else   => 0.8   // Poor, accept less

// Generate asks across wealth percentiles
for p in [0.1, 0.2, ..., 1.0]:
    reservation = 20 * (1 + wealth_factor * sqrt(p))
    quantity = population * 0.6 * 0.1  // 10% of labor force per bucket
    asks.push(Ask { Labor, quantity, min_price: reservation })
```

**Goods bids (provisions):**
```
Inputs: wealth, stockpile_provisions, target_provisions, market_price
Output: Vec<Bid> for Provisions

wealth_ratio = wealth / target_wealth
urgency = (target_provisions - stockpile_provisions) / target_provisions

spend_willingness = match wealth_ratio:
    > 1.5  => 0.4
    > 0.8  => 0.25
    > 0.3  => 0.15
    else   => 0.08

budget = wealth * spend_willingness * (0.7 + urgency * 0.2)

// Bid at multiple price levels
for mult in [1.5, 1.2, 1.0, 0.9]:
    price = market_price * mult
    qty = min(budget / price, shortfall)
    bids.push(Bid { Provisions, qty, max_price: price })
    budget -= qty * price
```

**Goods bids (cloth):**
Similar to provisions but:
- Lower priority (gets remaining budget)
- Urgency based on cloth stockpile
- Non-essential, so more price-sensitive

**Consumption (from stockpile):**
```
base_need = population / 100
stockpile_ratio = stockpile / target_stockpile
consumption = base_need * min(1.1, sqrt(stockpile_ratio))
```

### Settlement Org (Hardcoded)

Settlement org has fixed, simple behavior.

**Labor:** Does not bid for labor (subsistence farm is fallback, not auction participant)

**Subsistence farm operation:**
```
After labor auction:
    unassigned = labor_supply - labor_assigned
    output = subsistence_output_curve(unassigned, population)
    wages = unassigned * 20

    treasury -= wages
    stockpile.add(Provisions, output)
    population.wealth += wages
```

**Goods asks (selling subsistence output):**
```
// Sell all provisions at market price (no holding)
provisions = stockpile.get(Provisions)
market_price = get_market_price(Provisions)

asks.push(Ask {
    Provisions,
    quantity: provisions,
    min_price: market_price * 0.9  // Willing to sell slightly below market
})
```

**Goods bids:** Settlement org does not buy goods.

### Regular Orgs (AI-Controlled)

Regular orgs have strategic behavior. Default AI provides sensible heuristics; smarter AIs can override.

#### Default AI: Facility Labor Bids

```
for facility in org.facilities:
    recipe = get_recipe(facility.kind)
    stockpile = get_stockpile(org, facility.location)

    // Check input availability
    input_eff = min(stockpile.get(input) / (recipe.base * ratio) for (input, ratio) in recipe.inputs)
    if input_eff == 0:
        continue  // No inputs, no labor demand

    // Calculate MVP curve
    output_price = get_market_price(facility.location, recipe.output)
    input_cost = sum(get_market_price(facility.location, g) * r for (g, r) in recipe.inputs)

    effective_workers = facility.optimal_workforce * input_eff

    // Submit bids at different MVP levels (diminishing returns)
    for batch_start in [0, 0.25, 0.5, 0.75]:
        batch_end = batch_start + 0.25
        workers_in_batch = effective_workers * 0.25
        marginal_output = recipe.base_output / facility.optimal_workforce
        // Apply diminishing returns if over optimal
        if batch_end > 1.0:
            marginal_output *= 0.7^(batch_end - 1.0)

        mvp = marginal_output * (output_price - input_cost)

        if mvp > 20:  // Only bid above subsistence
            bids.push(Bid { Labor, workers_in_batch, max_price: mvp })
```

#### Default AI: Input Purchasing

```
for facility in org.facilities:
    recipe = get_recipe(facility.kind)
    stockpile = get_stockpile(org, facility.location)

    for (input_good, ratio) in recipe.inputs:
        current = stockpile.get(input_good)
        target = recipe.base_output * ratio * INPUT_TARGET_TICKS  // e.g., 5

        shortfall = target - current
        if shortfall <= 0:
            continue

        treasury_ratio = org.treasury / org.target_treasury
        shortfall_ratio = shortfall / target

        // Buy urgency curve
        urgency = match (treasury_ratio, shortfall_ratio):
            (> 1.5, _)      => shortfall_ratio * 1.0
            (> 0.8, _)      => shortfall_ratio * 0.7
            (> 0.3, _)      => shortfall_ratio * 0.4
            _               => shortfall_ratio * 0.1

        // Max price curve
        max_mult = match (treasury_ratio, shortfall_ratio):
            (> 1.0, > 0.8)  => 1.5   // Desperate and rich
            (_, > 0.5)      => 1.2   // Moderately urgent
            _               => 1.0   // Only market price

        market_price = get_market_price(facility.location, input_good)

        bids.push(Bid {
            input_good,
            quantity: shortfall * urgency,
            max_price: market_price * max_mult
        })
```

#### Default AI: Output Selling

```
for facility in org.facilities:
    recipe = get_recipe(facility.kind)
    stockpile = get_stockpile(org, facility.location)

    output_good = recipe.output
    current = stockpile.get(output_good)
    target = recipe.base_output * OUTPUT_TARGET_TICKS  // e.g., 2

    surplus = current - target
    if surplus <= 0:
        continue

    treasury_ratio = org.treasury / org.target_treasury
    surplus_ratio = surplus / target

    // Sell urgency curve
    urgency = match (treasury_ratio, surplus_ratio):
        (< 0.3, _)      => 1.0   // Liquidate, need cash
        (< 0.8, _)      => 0.8
        (_, > 1.0)      => 0.7   // Clear excess
        _               => 0.4

    // Min price curve
    min_mult = match (treasury_ratio, surplus_ratio):
        (< 0.3, _)      => 0.7   // Desperate
        (_, > 2.0)      => 0.8   // Need to clear
        _               => 0.95  // Hold for fair price

    market_price = get_market_price(facility.location, output_good)

    asks.push(Ask {
        output_good,
        quantity: surplus * urgency,
        min_price: market_price * min_mult
    })
```

#### Default AI: Ship Routing

```
for ship in org.ships:
    if ship.status != InPort:
        continue

    ship_stockpile = get_stockpile(org, Ship(ship.id))
    port_stockpile = get_stockpile(org, Settlement(ship.location))

    // Find best arbitrage opportunity
    best_score = 0
    best_destination = None
    best_cargo = []

    for destination in reachable_settlements(ship.location):
        for good in tradeable_goods():
            local_price = get_market_price(ship.location, good)
            dest_price = get_market_price(destination, good)

            available = port_stockpile.get(good) + ship_stockpile.get(good)
            if available < 1:
                continue

            margin = dest_price - local_price
            if margin <= 0:
                continue

            score = margin * min(available, ship.capacity)
            if score > best_score:
                best_score = score
                best_destination = destination
                best_cargo = [(good, min(available, ship.capacity))]

    if best_destination:
        // Transfer cargo from port stockpile to ship if needed
        // Then depart
        send_ship(ship, best_destination, best_cargo)
    else:
        // No opportunity, stay in port (or reposition)
        pass
```

#### Default AI: Ship Trading at Port

Ships can buy/sell independently of facilities:

```
for ship in org.ships:
    if ship.status != InPort:
        continue

    ship_stockpile = get_stockpile(org, Ship(ship.id))

    // Sell cargo that's expensive here
    for (good, qty) in ship_stockpile.goods:
        local_price = get_market_price(ship.location, good)
        // Check if price is good (compare to other settlements)
        if is_high_price(good, local_price):
            asks.push(Ask {
                good,
                quantity: qty,
                min_price: local_price * 0.95
            })

    // Buy goods that are cheap here
    remaining_capacity = ship.capacity - ship_stockpile.total()
    if remaining_capacity > 0:
        for good in tradeable_goods():
            local_price = get_market_price(ship.location, good)
            if is_low_price(good, local_price):
                budget = org.treasury * 0.2  // Limit spending per port
                qty = min(remaining_capacity, budget / local_price)
                bids.push(Bid {
                    good,
                    quantity: qty,
                    max_price: local_price * 1.05
                })
```

## Smarter AI Strategies (Future)

The default AI is reactive. Smarter strategies:

### Anticipation
- Track production/consumption trends
- Buy inputs before shortages hit
- Position ships ahead of demand

### Coordination
- Avoid sending multiple ships to same opportunity
- Coordinate production across settlements
- Balance inventory across locations

### Market Making
- Hold inventory to stabilize prices
- Buy low / sell high over time
- Create artificial scarcity (ruthless)

### Information Exploitation
- React faster to price changes
- Arbitrage before competitors
- (Future: limited information makes this meaningful)

## Decision Points Summary

| Entity | Decision | Timing | Method |
|--------|----------|--------|--------|
| Population | Labor asks | Before labor auction | Wealth-based reservation curve |
| Population | Goods bids | Before goods auction | Demand curve + budget |
| Population | Consumption | After goods auction | Rationing curve from stockpile |
| Settlement Org | Subsistence wages | After labor auction | Fixed: unassigned * 20 |
| Settlement Org | Goods asks | Before goods auction | Sell all output at ~market price |
| Regular Org | Labor bids | Before labor auction | MVP curve per facility |
| Regular Org | Input bids | Before goods auction | Shortfall + treasury curves |
| Regular Org | Output asks | Before goods auction | Surplus + treasury curves |
| Regular Org | Ship routing | After auctions | Arbitrage opportunity scoring |
| Regular Org | Ship trading | During goods auction | Buy low, sell high at port |

## What AIs Cannot Do (v1)

- Build or demolish facilities
- Build, buy, or sell ships
- Set wages directly (emerge from auction)
- Set prices directly (emerge from auction)
- Hire/fire specific workers
- Access other orgs' private information

## Open Questions

All deferred to iteration:

1. **Learning** — Should AI adapt based on past outcomes?
2. **Personality** — Different AI profiles (aggressive, conservative, etc.)?
3. **Alliances** — Can AIs coordinate or compete explicitly?
4. **Player interface** — How does player issue commands vs AI defaults?
