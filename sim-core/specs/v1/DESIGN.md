# Manifest Simulation Design

## Design Intent

Manifest is an economic simulation set in the late 1700s. The core fantasy is running a trading organization in a pre-industrial, pre-telegraph world where:

- **Information travels physically** — You only know prices where your ships have been
- **Geography constrains commerce** — Water transport is cheap, land transport is expensive
- **Organizations compete through positioning** — Not combat, but better routes, better information, better capital allocation

The simulation should feel like an **ecosystem**, not a spreadsheet. Prices emerge from supply and demand. Shortages cascade. Prosperity attracts population. The player observes and intervenes in a living system.

### Guiding Principles

1. **Closed-loop economics** — Every coin and every good is tracked. Money paid as wages becomes population income becomes purchases becomes seller revenue. No magic sources or sinks.

2. **Emergence over scripting** — Don't hardcode "this city is prosperous." Let prosperity emerge from good resources, good trade connections, adequate supply of goods.

3. **Simple rules, complex outcomes** — Individual mechanics should be easy to understand. Complexity comes from interactions.

4. **Legibility** — The player should be able to trace cause and effect. Why did prices spike? Because a ship was lost. Why is population declining? Because provisions are scarce.

---

## Entities

### Settlement

A node in the trade network. Represents a village, town, port, or city.

**Has:**
- Geographic position
- Population (the people who live here)
- Natural resources (what can be extracted here)
- Market (where goods are bought and sold)
- Labor market (where workers are hired)
- Warehouses (org-specific storage)
- Facilities (production buildings)

**Role:** Settlements are where economic activity happens. Production, consumption, and trade all occur at settlements. Routes connect settlements.

### Population

The aggregate inhabitants of a settlement. Not individually modeled — treated as a pool with characteristics.

**Has:**
- Count (number of people)
- Wealth (average savings per capita)
- Income (wages earned this tick)
- Stockpile (household provisions buffer)

**Role:** Population is both labor supply and consumption demand. They work at facilities, earn wages, and spend those wages on goods. Their wellbeing (fed, clothed) determines growth or decline.

### Organization (Org)

A player or AI-controlled economic actor. Represents a trading company, merchant family, noble estate, guild, etc.

**Has:**
- Treasury (liquid money)
- Known prices (information about distant markets)
- Owned facilities
- Owned ships

**Role:** Orgs make decisions — where to produce, where to trade, what to ship. They compete for profit by allocating capital and exploiting information advantages.

### Facility

A production asset at a settlement. Represents a farm, mine, mill, foundry, shipyard, etc.

**Has:**
- Type (determines inputs → outputs)
- Owner (an Org)
- Location (a Settlement)
- Optimal workforce (peak efficiency staffing)
- Current workforce (actual workers employed)
- Efficiency (output multiplier based on staffing, tools, etc.)

**Role:** Facilities transform inputs into outputs. A mill turns grain into flour. A foundry turns ore into iron. They employ workers (paying wages) and consume inputs (from owner's warehouse or market).

### Ship

A transport asset that moves goods between settlements.

**Has:**
- Owner (an Org)
- Capacity (cargo limit)
- Cargo (current inventory)
- Status (in port / en route)
- Location (current settlement if in port)
- Destination and days remaining (if en route)

**Role:** Ships are how goods (and information) move. When a ship arrives at a port, it can unload cargo, load new cargo, and the owning org learns current market prices.

### Route

A connection between two settlements.

**Has:**
- Endpoints (two settlements)
- Mode (sea / river / land)
- Distance (travel time in days)
- Risk (chance of incident per trip)

**Role:** Routes define the trade network topology. Sea routes are fast and cheap but require ports. River routes follow waterways. Land routes are slow and expensive but go anywhere.

### Market

The trading mechanism at a settlement. One per settlement.

**Has:**
- Per-good state: supply available, current price, recent demand
- Clearing mechanism

**Role:** Markets are where orgs sell goods and population buys goods. Prices emerge from the clearing process — high demand relative to supply drives prices up, oversupply drives prices down.

### Labor Market

The employment mechanism at a settlement. One per settlement.

**Has:**
- Labor supply (workers available)
- Labor demand (workers wanted by facilities)
- Wage (clearing price for labor)

**Role:** Labor markets determine wages. Facilities compete for workers. High demand for labor drives wages up (good for population, costly for orgs). Unemployment drives wages down.

---

## Goods

Goods flow through a production chain from raw resources to finished products.

### Tiers

```
PRIMARY (extraction)     PROCESSED (transformation)    FINISHED (consumption)
─────────────────────    ─────────────────────────    ─────────────────────
Grain ──────────────────► Flour ─────────┐
                                         ├──────────► Provisions
Fish ────────────────────────────────────┘

Timber ─────────────────► Lumber ────────┬──────────► Tools
                                         │
Ore ────────────────────► Iron ──────────┤
                                         │
Wool ───────────────────► Cloth ─────────┴──────────► Ships (capital good)
```

### Good Properties

| Good | Family | Notes |
|------|--------|-------|
| Grain | Primary | Requires fertile land, seasonal |
| Fish | Primary | Requires fishery access |
| Timber | Primary | Requires forest |
| Ore | Primary | Requires ore deposit |
| Wool | Primary | Requires pastureland |
| Flour | Processed | Grain → Flour at mill |
| Lumber | Processed | Timber → Lumber at lumber camp |
| Iron | Processed | Ore → Iron at foundry |
| Cloth | Processed | Wool → Cloth at weaver |
| Provisions | Finished | Flour + Fish → Provisions (food people eat) |
| Tools | Finished | Lumber + Iron → Tools (consumed by facilities) |
| Ships | Capital | Lumber + Iron + Cloth → Ships (can be used or sold) |

### Why This Graph

- **Lumber is a bottleneck** — needed for tools AND ships
- **Iron is contested** — tools need it, ships need it
- **Provisions require processing** — you can't just eat raw grain
- **Tools create circular demand** — facilities consume tools to maintain efficiency
- **Ships are both product and capital** — can sell them or use them yourself

---

## Economic Mechanics

### The Tick Cycle

Each tick represents a fixed time period (e.g., one week). The following phases execute in order:

#### Phase 1: Labor Market Clearing

For each settlement:

1. **Calculate labor supply** — Population available to work (most of population, minus some minimum idle/domestic)

2. **Calculate labor demand** — Sum of desired workforce across all facilities at this settlement

3. **Clear the market** — Find wage where supply ≈ demand
   - If demand > supply: wage rises (workers are scarce)
   - If supply > demand: wage falls (unemployment)

4. **Allocate workers** — Distribute available workers to facilities
   - Allocation method: proportional to demand, or priority queue, or auction (TBD)
   - Facilities may be understaffed if labor is scarce

5. **Pay wages** — Facility owners pay wages from treasury to population income
   - Wage bill = workers × wage rate
   - Money flows: Org treasury → Population income

#### Phase 2: Production

For each facility:

1. **Calculate efficiency** — Based on workforce and tool supply
   - Understaffed: efficiency = current_workers / optimal_workers
   - Overstaffed: diminishing returns curve
   - Missing tools: efficiency penalty

2. **Consume inputs** — Take required inputs from owner's warehouse
   - If inputs unavailable: production reduced proportionally
   - Tools consumed for maintenance (slow drain)

3. **Produce outputs** — Create goods based on facility type, efficiency, and inputs
   - Output goes to owner's warehouse at this settlement

#### Phase 3: Goods Market Clearing

For each settlement, for each good:

1. **Collect sell orders** — Orgs offer goods from their warehouses
   - For v1: orgs auto-sell everything (no strategic holding)
   - Later: orgs can set reserve prices or hold inventory

2. **Collect buy orders** —
   - Population demands provisions, cloth (based on need and purchasing power)
   - Facilities demand inputs (through owner, constrained by treasury)

3. **Find clearing price** —
   - If demand > supply: price rises
   - If supply > demand: price falls
   - Price adjusts gradually (10-20% per tick max) to prevent wild swings

4. **Execute trades** —
   - Quantity traded = min(supply, demand)
   - Goods move: seller warehouse → buyer (population or org warehouse)
   - Money moves: buyer (population income or org treasury) → seller treasury

5. **Record unmet demand** — Track what couldn't be bought (for effects phase)

#### Phase 4: Consumption Effects

1. **Population consumes purchased goods** —
   - Provisions consumed → if insufficient, health/growth penalty
   - Cloth consumed → if insufficient, wealth penalty (making do)
   - Tools consumed by facilities → if insufficient, efficiency penalty next tick

2. **Update population wealth** —
   - Surplus income (after purchases) → savings (wealth increases)
   - Deficit (couldn't afford needs) → wealth drawn down
   - Sustained deficit → wealth depletes, population decline

3. **Update population health/growth** —
   - Well-fed population grows slowly
   - Starving population declines
   - Wealthy population attracts immigration (from other settlements)

#### Phase 5: Transport

1. **Ships en route** — Decrement days remaining
   - If days remaining reaches 0 → ship arrives

2. **Ships arriving** —
   - Unload cargo to owner's warehouse at destination
   - Owner acquires price snapshot for this market
   - Risk check: chance of cargo loss, ship damage (if implemented)

3. **Ships in port** —
   - Available for loading new cargo
   - Available for departure orders

4. **Process departure orders** —
   - Load cargo from warehouse to ship (up to capacity)
   - Set destination and calculate travel time from route
   - Ship status → en route

#### Phase 6: Information Propagation

1. **Ships arriving share snapshots** — Org learns prices at destination market

2. **Snapshot aging** — Existing snapshots become more stale (track tick acquired)

3. **Information within org hierarchy** — (for later: subordinate orgs share up)

---

### Market Clearing Details

The clearing mechanism should produce intuitive results:

**Price adjustment formula:**
```
supply_demand_ratio = demand / max(supply, small_epsilon)
price_adjustment = ln(ratio) × adjustment_speed
new_price = old_price × (1 + clamp(price_adjustment, -max_change, +max_change))
```

Where:
- `adjustment_speed` ≈ 0.1 (10% base adjustment)
- `max_change` ≈ 0.2 (20% max change per tick)
- `small_epsilon` prevents division by zero

**Intuition:**
- ratio = 2 (demand is 2x supply) → price up ~7%
- ratio = 0.5 (supply is 2x demand) → price down ~7%
- ratio = 1 (balanced) → price stable

**Price bounds:**
- Minimum price: 1 (goods have some base value)
- Maximum price: 1000 (even extreme scarcity has limits)

### Labor Market Details

Similar clearing mechanism to goods, but for wages:

**Wage adjustment:**
```
employment_ratio = labor_demand / max(labor_supply, epsilon)
wage_adjustment = ln(ratio) × adjustment_speed
new_wage = old_wage × (1 + clamp(wage_adjustment, -max_change, +max_change))
```

**Worker allocation when demand > supply:**

Option A: Proportional — each facility gets workers proportional to its demand
Option B: Priority — facilities sorted by wage offered (but we have uniform wage...)
Option C: Random — shuffle and fill until workers exhausted

For v1: Proportional allocation is simplest and fair.

### Consumption Demand

Population demand for goods depends on need and ability to pay:

**Provisions (inelastic):**
```
base_need = population_count / 100  // ~1 unit per 100 people per tick
demand = base_need × demand_curve(price)
// demand_curve: slight reduction at high prices but people need to eat
```

**Cloth (elastic):**
```
base_want = population_count × wealth / 500
demand = base_want × demand_curve(price)
// demand_curve: significant reduction at high prices (luxury)
```

**Purchasing power constraint:**
```
max_spend = population_income + (population_wealth × drawdown_rate)
// People can spend income plus dip into savings if needed
```

If total cost of demanded goods exceeds purchasing power, reduce quantities proportionally (prioritizing provisions).

---

## Facility Types

| Type | Inputs | Outputs | Required Resource | Notes |
|------|--------|---------|-------------------|-------|
| Farm | (labor) | Grain | Fertile Land | Seasonal pulse? |
| Fishery | (labor) | Fish | Fishery | Steady output |
| Lumber Camp | (labor) | Timber/Lumber | Forest | |
| Mine | (labor) | Ore | Ore Deposit | |
| Pasture | (labor) | Wool | Pastureland | |
| Mill | Grain | Flour | — | Needs water? |
| Foundry | Ore | Iron | — | |
| Weaver | Wool | Cloth | — | |
| Bakery/Provisioner | Flour, Fish | Provisions | — | |
| Toolsmith | Lumber, Iron | Tools | — | |
| Shipyard | Lumber, Iron, Cloth | Ships | — | Coastal only? |

### Production Formulas

Each facility type has:
- `base_output`: units produced per tick at full efficiency with full workforce
- `input_ratios`: units of each input consumed per unit of output
- `labor_intensity`: workers needed for full capacity

Example (Mill):
```
base_output: 10 flour/tick
inputs: 12 grain per 10 flour (1.2:1 ratio)
optimal_workforce: 5
```

Actual output:
```
workforce_efficiency = calculate_from_staffing()
input_efficiency = min(available_input / required_input, 1.0) for each input
tool_efficiency = calculate_from_tool_supply()

output = base_output × workforce_efficiency × input_efficiency × tool_efficiency
```

---

## Expected Dynamics

### Price Equilibration

If Settlement A has cheap grain and Settlement B has expensive grain:
1. Trader notices price differential (information)
2. Ships grain from A to B
3. A's supply decreases → A's price rises
4. B's supply increases → B's price falls
5. Prices converge (minus transport cost)

**Equilibrium:** Price difference ≈ transport cost + risk premium + profit margin

### Production Chain Cascades

If iron becomes scarce:
1. Tool production falls (missing input)
2. Tool prices rise
3. Facilities without tools lose efficiency
4. All production falls
5. Goods prices rise across the board
6. Population struggles, wealth declines

### Wage-Price Spiral

If population grows faster than production:
1. More workers available → wages fall
2. But more consumers → demand rises
3. Prices rise (demand > supply)
4. Real wages (purchasing power) falls
5. Population growth slows (can't afford provisions)
6. System finds new equilibrium

### Information Advantage

If you have faster ships or better positioned factors:
1. You learn about price spikes first
2. You can ship goods before competitors
3. You capture arbitrage profit
4. Competitors arrive to find prices already equalized

---

## Open Questions for Later

1. **Seasonality** — Should grain production pulse annually? Creates storage gameplay.

2. **Risk and loss** — Ships lost at sea, piracy, cargo damage. How punishing?

3. **Org hierarchy and contracts** — The design doc's vision of arbiters, reputation cascades. Complex but flavorful.

4. **Strategic selling** — Orgs holding inventory for better prices, speculation.

5. **Land transport** — Currently routes exist but caravans aren't modeled like ships.

6. **Credit and debt** — Orgs borrowing against future income. Adds leverage and risk.

7. **Org-affiliated population** — The richer model where orgs have their own people.

8. **Natural events** — Storms, plagues, bumper harvests. External shocks.

---

## v1 Scope

For the first working implementation:

**Include:**
- 5-7 settlements with varied resources
- All 11 goods (full production chain)
- Settlement-level population with income/wealth
- Labor market with clearing wages
- Goods market with clearing prices
- Facilities with inputs/outputs/labor
- Ships that move cargo and carry price information
- One player org, one simple AI org

**Exclude (for now):**
- Seasonality
- Ship loss/risk (ships always arrive)
- Org hierarchies and contracts
- Strategic inventory holding (auto-sell)
- Land caravans (sea/river routes only)
- Credit/debt
- Org-affiliated population
- Events/shocks
