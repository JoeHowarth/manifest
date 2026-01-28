# Auctions Specification (v2)

## Overview

All markets (labor and goods) use the same auction mechanism. Participants submit bids (to buy) and asks (to sell), and the market clears by matching them.

**Key principles:**
- **One mechanism for everything** — labor and goods use identical clearing logic
- **Bids and asks from curves** — demand/supply curves converted to discrete orders
- **Price discovery** — clearing price emerges from matching
- **Partial fills** — orders can be partially filled

## Bid and Ask Structure

```
Bid {
    buyer: EntityId,       // OrgId or PopulationId
    good: GoodType,        // Including Labor
    quantity: f32,
    max_price: f32,        // Won't pay more than this
}

Ask {
    seller: EntityId,      // OrgId or PopulationId
    good: GoodType,
    quantity: f32,
    min_price: f32,        // Won't accept less than this
}

EntityId {
    Org(OrgId),
    Population(SettlementId),
}
```

## Clearing Mechanism

For each (settlement, good) market:

```
function clear_market(bids: Vec<Bid>, asks: Vec<Ask>) -> Vec<Transaction>:
    // Sort for matching
    asks.sort_by(|a| a.min_price)           // Lowest ask first
    bids.sort_by(|b| -b.max_price)          // Highest bid first

    transactions = []
    ask_idx = 0
    bid_idx = 0

    while ask_idx < asks.len() and bid_idx < bids.len():
        ask = asks[ask_idx]
        bid = bids[bid_idx]

        // Check if trade is possible
        if bid.max_price < ask.min_price:
            break  // No more profitable matches

        // Determine quantity and price
        quantity = min(ask.quantity, bid.quantity)
        price = (ask.min_price + bid.max_price) / 2  // Split the surplus

        // Record transaction
        transactions.push(Transaction {
            seller: ask.seller,
            buyer: bid.buyer,
            good: ask.good,
            quantity: quantity,
            price: price,
        })

        // Update remaining quantities
        ask.quantity -= quantity
        bid.quantity -= quantity

        // Move to next order if filled
        if ask.quantity <= 0:
            ask_idx += 1
        if bid.quantity <= 0:
            bid_idx += 1

    return transactions
```

### Transaction Execution

```
Transaction {
    seller: EntityId,
    buyer: EntityId,
    good: GoodType,
    quantity: f32,
    price: f32,
}

function execute(tx: Transaction):
    total_cost = tx.quantity * tx.price

    if tx.good == Labor:
        // Labor: assign workers, pay wages
        assign_workers(tx.buyer, tx.seller, tx.quantity)
        transfer_money(tx.buyer.treasury, tx.seller.wealth, total_cost)
    else:
        // Goods: transfer inventory and money
        transfer_goods(tx.seller.stockpile, tx.buyer.stockpile, tx.good, tx.quantity)
        transfer_money(tx.buyer.treasury, tx.seller.treasury, total_cost)
```

## Labor Market

### Labor Asks (Population Selling Labor)

Population generates asks based on reservation wage curve:

```
function generate_labor_asks(population: Population, settlement: Settlement) -> Vec<Ask>:
    asks = []
    labor_pool = population.count * labor_force_rate  // e.g., 0.6

    // Wealth affects reservation wages
    wealth_ratio = population.wealth / population.target_wealth
    wealth_factor = reservation_curve(wealth_ratio)
    // wealth_ratio > 1.5 → wealth_factor ≈ 1.5 (demand premium)
    // wealth_ratio < 0.5 → wealth_factor ≈ 0.8 (accept less)

    // Generate asks at different price levels
    // Poorest workers have lowest reservation, wealthiest have highest
    for percentile in [0.1, 0.2, 0.3, ..., 1.0]:
        reservation = SUBSISTENCE_WAGE * (1 + wealth_factor * percentile^0.5)
        workers_at_level = labor_pool * 0.1  // 10% of labor pool per bucket

        asks.push(Ask {
            seller: Population(settlement.id),
            good: Labor,
            quantity: workers_at_level,
            min_price: reservation,
        })

    return asks
```

### Labor Bids (Facilities Buying Labor)

Facilities generate bids based on marginal value product:

```
function generate_labor_bids(facility: Facility, settlement: Settlement) -> Vec<Bid>:
    bids = []
    recipe = get_recipe(facility.kind)

    // Check input availability
    stockpile = get_stockpile(facility.owner, settlement)
    input_efficiency = calculate_input_efficiency(recipe, stockpile)

    if input_efficiency == 0:
        return []  // No inputs, no labor demand

    // Calculate MVP for different workforce levels
    output_price = get_market_price(settlement, recipe.output)
    input_cost = calculate_input_cost(recipe, settlement)

    // Marginal output per worker (with diminishing returns beyond optimal)
    for worker_batch in batches(facility.optimal_workforce * input_efficiency):
        marginal_output = calculate_marginal_output(facility, worker_batch)
        mvp = marginal_output * output_price - marginal_output * input_cost

        if mvp > SUBSISTENCE_WAGE:  // Only bid if profitable above subsistence
            bids.push(Bid {
                buyer: Org(facility.owner),
                good: Labor,
                quantity: worker_batch.size,
                max_price: mvp,
            })

    return bids
```

### Subsistence Farm Special Handling

Subsistence farm doesn't participate in auction — it's the fallback:

```
After labor auction clears:
    unassigned_workers = total_labor_supply - total_labor_assigned

    // All unassigned workers go to subsistence
    subsistence_workers = unassigned_workers
    subsistence_output = calculate_diminishing_output(subsistence_workers, population)
    subsistence_wages = subsistence_workers * SUBSISTENCE_WAGE  // Fixed at 20

    // Settlement org pays wages, receives output
    settlement_org.treasury -= subsistence_wages
    settlement_org.stockpile.add(Provisions, subsistence_output)
    population.wealth += subsistence_wages
```

## Goods Market

### Goods Asks (Orgs Selling)

Orgs generate asks based on inventory and targets:

```
function generate_goods_asks(org: Org, settlement: Settlement) -> Vec<Ask>:
    asks = []
    stockpile = get_stockpile(org, settlement)

    for good in stockpile.goods.keys():
        current = stockpile.get(good)
        target = calculate_target_inventory(org, settlement, good)
        surplus = current - target

        if surplus <= 0:
            continue

        // Min price based on treasury health and inventory pressure
        treasury_ratio = org.treasury / org.target_treasury
        inventory_ratio = current / target

        min_price_mult = selling_price_curve(treasury_ratio, inventory_ratio)
        market_price = get_market_price(settlement, good)

        asks.push(Ask {
            seller: Org(org.id),
            good: good,
            quantity: surplus * sell_urgency(treasury_ratio, inventory_ratio),
            min_price: market_price * min_price_mult,
        })

    return asks
```

### Goods Bids (Orgs Buying Inputs)

Orgs generate bids for facility inputs:

```
function generate_input_bids(org: Org, settlement: Settlement) -> Vec<Bid>:
    bids = []
    stockpile = get_stockpile(org, settlement)

    for facility in org.facilities_at(settlement):
        recipe = get_recipe(facility.kind)

        for (input_good, ratio) in recipe.inputs:
            current = stockpile.get(input_good)
            target = recipe.base_output * ratio * org.input_target_ticks
            shortfall = target - current

            if shortfall <= 0:
                continue

            // Max price based on treasury and urgency
            treasury_ratio = org.treasury / org.target_treasury
            shortfall_ratio = shortfall / target

            max_price_mult = buying_price_curve(treasury_ratio, shortfall_ratio)
            market_price = get_market_price(settlement, input_good)

            bids.push(Bid {
                buyer: Org(org.id),
                good: input_good,
                quantity: shortfall * buy_urgency(treasury_ratio, shortfall_ratio),
                max_price: market_price * max_price_mult,
            })

    return bids
```

### Population Bids (Buying Provisions, Cloth)

Population generates bids based on demand curves and budget:

```
function generate_population_bids(population: Population, settlement: Settlement) -> Vec<Bid>:
    bids = []

    // Calculate budget from wealth
    wealth_ratio = population.wealth / population.target_wealth
    spend_willingness = spending_curve(wealth_ratio)
    total_budget = population.wealth * spend_willingness

    // Provisions (essential)
    provisions_urgency = (population.target_provisions - population.stockpile_provisions)
                         / population.target_provisions
    provisions_budget = total_budget * (0.7 + provisions_urgency * 0.2)

    // Generate bids at decreasing price levels
    remaining_budget = provisions_budget
    for price_mult in [1.5, 1.2, 1.0, 0.9, 0.8]:
        price = get_market_price(settlement, Provisions) * price_mult
        quantity_affordable = remaining_budget / price
        quantity_wanted = calculate_provisions_demand(population, price)
        quantity = min(quantity_affordable, quantity_wanted)

        if quantity > 0:
            bids.push(Bid {
                buyer: Population(settlement.id),
                good: Provisions,
                quantity: quantity,
                max_price: price,
            })
            remaining_budget -= quantity * price

    // Similar for cloth (with lower priority)
    // ...

    return bids
```

## Price Discovery

### Market Price Tracking

Each settlement tracks prices for reference:

```
MarketPrice {
    good: GoodType,
    last_price: f32,          // Price from last transaction
    volume_weighted_avg: f32,  // Recent average
    last_traded_quantity: f32,
}
```

Updated after each auction:
```
for transaction in transactions:
    market_price[transaction.good].update(transaction.price, transaction.quantity)
```

### Initial/Default Prices

When no transactions have occurred:
```
default_price(good) = match good {
    Provisions => 20,  // Anchored to subsistence wage
    Labor => 20,       // Subsistence wage
    Grain => 15,
    Flour => 18,
    // ... etc
}
```

## Numeric Anchoring

The subsistence wage (20) anchors the entire economy:

```
SUBSISTENCE_WAGE = 20

// Labor floor: no one works for less than subsistence
min_labor_price = SUBSISTENCE_WAGE

// Provisions anchor: subsistence workers earn 20, need ~1 provision
// So provisions price gravitates toward ~20
// If price >> 20: workers can't afford → demand falls → price falls
// If price << 20: surplus → population grows → demand rises → price rises
```

## Hysteresis

To prevent oscillation, price expectations change gradually:

```
function update_price_expectation(current: f32, observed: f32) -> f32:
    change = observed - current
    if abs(change) < threshold:
        return current  // Ignore small changes
    adjustment = clamp(change * rate, -max_change, +max_change)
    return current + adjustment
```

Suggested parameters:
- `rate`: 0.3 (30% adjustment toward observed)
- `threshold`: 1.0 (ignore changes < 1.0)
- `max_change`: 5.0 (cap large swings)

## Market Sequence

Each tick, markets clear in this order:

```
1. LABOR MARKET (per settlement)
   - Collect labor asks from population
   - Collect labor bids from facilities
   - Clear auction
   - Assign workers, pay wages
   - Unassigned → subsistence farm

2. GOODS MARKETS (per settlement, per good)
   - Collect asks from orgs (selling)
   - Collect bids from orgs (buying inputs)
   - Collect bids from population (buying consumption goods)
   - Collect asks from settlement org (selling subsistence output)
   - Clear auction
   - Execute transfers
```

## Resolved Design Decisions

1. **Ship trading** — Ships have separate stockpile from warehouse.
   - Ship generates bids/asks from its **own cargo hold**
   - Transfer between ship hold and org warehouse at same port is **free but explicit**
   - Ship can: (a) trade directly from hold, or (b) transfer to warehouse first, then trade
   - This allows ships to act as mobile traders independent of local facilities

2. **Same-entity trades** — Settlement org sells to its own population. This is expected and correct (subsistence workers buy back the food they produced via the market).

## Open Questions

1. **Price for unmatched orders** — If an ask goes unfilled, at what price do we record it for market price tracking? Probably just don't update.

2. **Order of goods markets** — Does it matter which good clears first? Probably not if all orders are submitted before any clearing.
