# Simulation Specs v2

Unified economic model with auctions for everything.

## Core Documents

1. **[ENTITIES.md](./ENTITIES.md)** — Core entities and relationships
   - Orgs (including Settlements as special orgs)
   - Facilities, Ships, Population
   - Stockpiles at locations
   - Goods (including Labor as a tradeable good)

2. **[AUCTIONS.md](./AUCTIONS.md)** — Unified market clearing
   - Bid/ask structure
   - Clearing mechanism (same for labor and goods)
   - How each entity generates orders
   - Price discovery and anchoring

3. **[AI_BEHAVIOR.md](./AI_BEHAVIOR.md)** — Decision-making logic
   - Population behavior (hardcoded curves)
   - Settlement org behavior (hardcoded)
   - Regular org AI (default heuristics)
   - Ship routing and trading

## Key Design Principles

### Unified Auction Model
- **Labor is a good** — same auction mechanism for workers and commodities
- **Everyone submits bids/asks** — population, orgs, settlement orgs
- **Price emerges from clearing** — no explicit price-setting

### Settlements as Orgs
- Settlement has an associated Settlement Org
- Settlement Org owns subsistence farm
- Participates in markets like any org
- Hardcoded AI (simple, predictable)

### Stockpiles at Locations
- Org has stockpile at each settlement where it operates
- Ship has stockpile (the cargo hold)
- Transfer at same location is free
- Transfer between locations requires shipping

### Numeric Anchoring
- Subsistence wage = 20 (the numeraire)
- Provisions price gravitates toward ~20
- All other prices float relative to this anchor

## Tick Sequence

```
1. PRODUCTION
   - Consume inputs → produce outputs

2. LABOR AUCTION
   - Population submits labor asks
   - Facilities submit labor bids
   - Clear, assign workers, pay wages
   - Unassigned → subsistence farm (wage=20)

3. GOODS AUCTION
   - Orgs submit input bids, output asks
   - Population submits consumption bids
   - Settlement org submits provision asks
   - Clear, transfer goods and money

4. CONSUMPTION
   - Population consumes from stockpile
   - Growth/decline based on satisfaction

5. TRANSPORT
   - Ships travel, arrive, depart
```

## Changes from v1 Specs

| v1 | v2 |
|----|-----|
| Separate labor and goods market mechanics | Unified auction for everything |
| Facility priority for labor | Facilities submit multiple bids at different MVPs |
| Settlement as special entity | Settlement Org (just an org with hardcoded AI) |
| Warehouse per org per settlement | Stockpile at LocationId (settlement or ship) |
| Subsistence as fallback with capacity limit | Subsistence as fallback with 100% capacity, fast diminishing returns |

## Key Design Decisions

1. **Population stockpile ≠ Org stockpile** — Separate systems. Population has household reserves; orgs have warehouse inventory. Goods flow through market between them.

2. **Subsistence flows through market** — Workers produce → settlement org stockpile → auction → population buys. Circular but consistent with auction model.

3. **Ship stockpile is separate** — Ship hold ≠ org warehouse. Transfer at same port is free but explicit. Ships can trade directly from hold.

## Open Questions

See individual docs for detailed open questions. Key ones:

1. **Initial conditions** — How to set up starting state (treasury, stockpiles, prices)?
2. **Settlement treasury** — Can it go negative? Initial funding?
