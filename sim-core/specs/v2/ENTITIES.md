# Entities Specification (v2)

## Overview

This document defines the core entities in the simulation and their relationships.

**Key principles:**
- **Settlements are Orgs** — unified model, settlements are just orgs with hardcoded AI
- **Stockpiles at locations** — orgs have inventory at settlements and on ships
- **Labor is a good** — same auction mechanism for labor and goods

## Goods

All tradeable commodities, including labor:

```
GoodType {
    // Primary
    Grain,
    Fish,
    Timber,
    Ore,
    Wool,

    // Processed
    Flour,
    Lumber,
    Iron,
    Cloth,

    // Finished
    Provisions,
    Tools,

    // Capital
    Ships,

    // Special
    Labor,  // Traded via same auction mechanism
}
```

### Good Properties

| Good | Stockpiles | Shippable | Notes |
|------|------------|-----------|-------|
| Grain, Fish, etc. | Yes | Yes | Normal goods |
| Provisions | Yes | Yes | Essential for population |
| Cloth | Yes | Yes | Comfort good |
| Labor | No | No | Settlement-scoped, consumed immediately |

## Organizations (Orgs)

The primary economic actors. Includes both player/AI trading companies AND settlements.

```
Org {
    id: OrgId,
    name: String,
    treasury: f32,
    org_type: OrgType,
}

OrgType {
    Regular,      // Player or AI trading company
    Settlement,   // Special: owns subsistence farm, hardcoded behavior
}
```

### Regular Orgs

- Own facilities and ships
- Manage treasury and inventory
- AI or player controlled
- Submit bids/asks based on strategy

### Settlement Orgs

Every settlement has an associated Settlement Org that:
- Owns the Subsistence Farm facility
- Has hardcoded AI (always operates subsistence, sells provisions)
- Participates in markets like any org
- Can be its own counterparty (workers buy provisions from settlement)

## Settlements

Geographic nodes where economic activity happens.

```
Settlement {
    id: SettlementId,
    name: String,
    position: (f32, f32),
    org_id: OrgId,              // The Settlement Org
    population: Population,
    natural_resources: Vec<NaturalResource>,
}

NaturalResource {
    FertileLand,
    Fishery,
    Forest,
    OreDeposit,
    Pastureland,
}
```

A settlement IS NOT an org, but HAS an associated settlement org. This keeps the concepts clean:
- Settlement = place with population and resources
- Settlement Org = the economic actor that owns subsistence farm

## Population

The inhabitants of a settlement. Source of labor supply and consumption demand.

```
Population {
    count: u32,
    wealth: f32,                    // Money savings (the stock)
    stockpile_provisions: f32,      // Household food reserves
    stockpile_cloth: f32,           // Household cloth reserves
    target_wealth: f32,             // Comfortable savings level
    target_provisions: f32,         // Desired food buffer
    target_cloth: f32,              // Desired cloth buffer
}
```

### Population as Market Participant

Population participates in markets by generating bids and asks:

**Labor asks** (selling labor):
- Derived from reservation wage curve
- Curve shifts based on wealth (wealthy → higher reservations)
- See AUCTIONS.md for details

**Goods bids** (buying provisions, cloth):
- Derived from demand curve and budget
- Budget based on wealth and spending willingness
- See AUCTIONS.md for details

## Facilities

Production units owned by orgs, located at settlements.

```
Facility {
    id: FacilityId,
    kind: FacilityType,
    owner: OrgId,
    location: SettlementId,
    optimal_workforce: u32,
    efficiency: f32,            // Output multiplier (tools, etc.)
}

FacilityType {
    // Primary (extraction)
    Farm,           // → Grain (requires FertileLand)
    Fishery,        // → Fish (requires Fishery)
    LumberCamp,     // → Lumber (requires Forest)
    Mine,           // → Ore (requires OreDeposit)
    Pasture,        // → Wool (requires Pastureland)

    // Processing
    Mill,           // Grain → Flour
    Foundry,        // Ore → Iron
    Weaver,         // Wool → Cloth

    // Finished
    Bakery,         // Flour + Fish → Provisions
    Toolsmith,      // Lumber + Iron → Tools

    // Capital
    Shipyard,       // Lumber + Iron + Cloth → Ships

    // Special
    SubsistenceFarm,  // → Provisions (diminishing returns, no inputs)
}
```

### Subsistence Farm

Special facility owned by Settlement Org:

```
SubsistenceFarm:
    owner: Settlement Org
    capacity: 100% of population (no hard limit)
    wage: 20 (fixed, the economic anchor)
    inputs: none
    output: Provisions

    output_per_worker(n) = base / (1 + n / k)
    where k = sqrt(population) * factor

    // Fast diminishing returns means only X workers sustainable
    // X grows sub-linearly with population
```

### Recipes

```
Recipe {
    inputs: Vec<(GoodType, f32)>,  // (good, amount per unit output)
    output: GoodType,
    base_output: f32,
    optimal_workforce: u32,
}
```

## Stockpiles

Inventory of goods at a location. Orgs have stockpiles, ships have stockpiles.

```
Stockpile {
    owner: OrgId,
    location: LocationId,
    goods: HashMap<GoodType, f32>,
}

LocationId {
    Settlement(SettlementId),
    Ship(ShipId),
}
```

### Stockpile Rules

- Org has one stockpile per settlement where it operates (has facilities)
- Ship has its own stockpile (the hold)
- Transfer between org stockpile and ship stockpile at same settlement: free
- Transfer between settlements: requires ship transport
- Labor does NOT stockpile (consumed immediately)

## Ships

Mobile transport and trading units.

```
Ship {
    id: ShipId,
    name: String,
    owner: OrgId,
    capacity: f32,
    status: ShipStatus,
    location: SettlementId,       // Current port (if in port)
    destination: Option<SettlementId>,
    days_remaining: u32,
}

ShipStatus {
    InPort,
    EnRoute,
}
```

### Ship Stockpile

Ship has its own stockpile (the cargo hold):

```
ship_stockpile = Stockpile {
    owner: ship.owner,
    location: Ship(ship.id),
    goods: ...,
}
```

### Ship Operations

When in port, a ship can:
1. **Transfer** goods to/from org's settlement stockpile (free)
2. **Buy** from market → goods to ship stockpile
3. **Sell** to market → goods from ship stockpile
4. **Depart** to another settlement

## Routes

Connections between settlements for ship travel.

```
Route {
    from: SettlementId,
    to: SettlementId,
    mode: TransportMode,
    distance: u32,        // Travel time in ticks
    risk: f32,            // Future: chance of incident
}

TransportMode {
    Sea,
    River,
    Land,   // Future: caravans
}
```

## Entity Relationships

```
┌─────────────────────────────────────────────────────────────────┐
│                         SETTLEMENT                               │
│  ┌──────────┐    has    ┌────────────┐                          │
│  │Population│◄─────────►│ Settlement │                          │
│  └────┬─────┘           └─────┬──────┘                          │
│       │                       │                                  │
│       │ supplies labor        │ has org                         │
│       │ demands goods         ▼                                  │
│       │                 ┌───────────┐                           │
│       │                 │Settlement │  owns   ┌──────────────┐  │
│       │                 │   Org     │────────►│Subsistence   │  │
│       │                 └───────────┘         │Farm          │  │
│       │                       ▲               └──────────────┘  │
│       │                       │ participates                    │
│       ▼                       │                                  │
│  ┌─────────┐            ┌─────┴─────┐         ┌──────────────┐  │
│  │ MARKET  │◄───────────│  Regular  │────────►│ Facilities   │  │
│  │(auction)│            │   Orgs    │  owns   └──────────────┘  │
│  └─────────┘            └─────┬─────┘                           │
│                               │ owns                             │
│                               ▼                                  │
│                         ┌───────────┐                           │
│                         │   Ships   │◄──── stockpile (hold)     │
│                         └───────────┘                           │
└─────────────────────────────────────────────────────────────────┘
```

## Tick Sequence

```
1. PRODUCTION
   - Facilities consume inputs from stockpile
   - Facilities produce outputs to stockpile
   - (Subsistence farm produces provisions)

2. LABOR AUCTION
   - Population generates labor asks (reservation wages)
   - Facilities generate labor bids (MVP)
   - Clear auction, assign workers, pay wages
   - Unassigned workers → subsistence farm

3. GOODS AUCTION
   - Orgs generate asks (selling inventory)
   - Orgs generate bids (buying inputs)
   - Population generates bids (buying provisions, cloth)
   - Settlement org generates asks (selling subsistence provisions)
   - Clear auction, transfer goods and money

4. CONSUMPTION
   - Population consumes from stockpile (rationing curve)
   - Population effects (growth/decline based on satisfaction)

5. TRANSPORT
   - Ships in transit: decrement days remaining
   - Ships arriving: unload cargo
   - Process ship orders (load, depart)
```

## Resolved Design Decisions

1. **Population stockpile vs org stockpile** — **Separate systems.**
   - Population has household reserves (`stockpile_provisions`, `stockpile_cloth`)
   - Orgs have warehouse stockpiles (`Stockpile { owner, location }`)
   - Flow: org stockpile → market auction → population stockpile → consumption
   - These are different entity types with different purposes

2. **Subsistence provisions flow** — **Through the market.**
   - Subsistence farm produces → settlement org stockpile
   - Settlement org sells in goods auction
   - Population buys from auction into household stockpile
   - Circular but keeps auction model consistent

3. **Ship stockpile** — **Separate from warehouse, explicit transfer.**
   - Ship has its own stockpile (cargo hold)
   - Org has separate stockpile at each settlement (warehouse)
   - Transfer between ship and warehouse at same port: free but explicit
   - Ship can trade directly from its hold, or transfer first

## Open Questions

1. **Settlement treasury** — Where does initial treasury come from? Can it go negative temporarily? (Defer to implementation)

2. **Multiple orgs at same settlement** — Each has separate stockpile. They compete in same market. (Clear, no issue)
