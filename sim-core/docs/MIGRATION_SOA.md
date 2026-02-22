# Migration Plan: Per-Settlement SoA Architecture

## Goal

Replace the current `HashMap<PopId, Pop>` / `HashMap<FacilityId, Facility>` architecture with per-settlement storage that is cache-friendly, eliminates the extract/reinsert pattern, and enables future per-settlement parallelism via rayon.

## Current Pain Points

1. **Extract/reinsert ceremony** (world.rs:188-255) — pops and merchants are removed from global HashMaps, processed, then reinserted every settlement every tick
2. **HashMap-per-entity for hot fields** — `Pop.stocks`, `Pop.desired_consumption_ema`, `Pop.need_satisfaction` are all `HashMap<GoodId, f64>` with ~10 entries each. Hash overhead dominates actual data
3. **Linear scans for settlement membership** — `facilities_at_settlement()` scans all facilities; `merchants_at_settlement()` does the same then deduplicates
4. **Global labor phase fights the borrow checker** — iterates facilities, looks up merchants, looks up price_ema, all from `&mut self`

## Target Architecture

### SettlementState — owns all settlement-local entities

```rust
pub struct SettlementState {
    pub info: Settlement,               // name, position, resource_slots

    // Pops (dense Vec, not HashMap)
    pub pops: Vec<Pop>,
    // Index: PopId → position in pops vec (for O(1) lookup by ID)
    pub pop_index: HashMap<PopId, usize>,

    // Facilities (dense Vec)
    pub facilities: Vec<Facility>,
    pub facility_index: HashMap<FacilityId, usize>,

    // Per-settlement market state
    pub price_ema: HashMap<GoodId, Price>,
    pub facility_bid_states: HashMap<FacilityId, FacilityBidState>,
    pub subsistence_queue: Vec<PopId>,
    pub depth_multipliers: HashMap<GoodId, f64>,
}
```

### World — global state + settlement container

```rust
pub struct World {
    pub tick: u64,
    pub settlement_states: HashMap<SettlementId, SettlementState>,
    pub routes: Vec<Route>,

    // Merchants remain global (they span settlements)
    pub merchants: HashMap<MerchantId, MerchantAgent>,

    // Global labor state
    pub wage_ema: HashMap<SkillId, Price>,

    // Global config (immutable during tick)
    pub external_market: Option<ExternalMarketConfig>,
    pub subsistence_reservation: Option<SubsistenceReservationConfig>,
    pub mortality_grace_ticks: u64,

    // Global accounting
    pub outside_flow_totals: OutsideFlowTotals,
    pub stock_flow_history: Vec<TickStockFlow>,

    // ID counters
    next_settlement_id: u32,
    next_agent_id: u32,
    next_facility_id: u32,
}
```

### What moves into SettlementState vs. stays global

| Data | Current location | New location | Reason |
|------|-----------------|-------------|--------|
| Pops | `World.pops` | `SettlementState.pops` | Pops are settlement-bound, never move |
| Facilities | `World.facilities` | `SettlementState.facilities` | Facilities are settlement-bound |
| `price_ema` | `World.price_ema[(sid,gid)]` | `SettlementState.price_ema[gid]` | Only accessed per-settlement |
| `facility_bid_states` | `World.facility_bid_states` | `SettlementState.facility_bid_states` | Keyed by facility, which is per-settlement |
| `subsistence_queues` | `World.subsistence_queues[sid]` | `SettlementState.subsistence_queue` | Already per-settlement |
| `trade_depth_multipliers` | `World.trade_depth_multipliers[(sid,gid)]` | `SettlementState.depth_multipliers[gid]` | Already per-settlement-keyed |
| Merchants | `World.merchants` | **Stays global** | Merchants span settlements via stockpiles |
| `wage_ema` | `World.wage_ema` | **Stays global** | Labor market clears globally across settlements |
| External market config | `World.external_market` | **Stays global** | Immutable config |
| Routes | `World.routes` | **Stays global** | Cross-settlement by definition |

## Migration Steps

### Phase 1: Introduce SettlementState, move pops and facilities

This is the structural change. Every subsequent phase is a refactor within this structure.

**1a. Create `SettlementState` struct**

Define `SettlementState` with `pops: Vec<Pop>`, `facilities: Vec<Facility>`, plus the index HashMaps. Keep all the same entity types — no field changes to `Pop` or `Facility` yet.

**1b. Change `World` to hold `HashMap<SettlementId, SettlementState>`**

Remove `World.pops`, `World.facilities`, `World.settlements`. Replace with `World.settlement_states`. Move `price_ema`, `facility_bid_states`, `subsistence_queues`, `trade_depth_multipliers` into `SettlementState`.

**1c. Update World builder methods**

- `add_settlement()` creates a `SettlementState`
- `add_pop(settlement_id)` pushes to `settlement_states[settlement_id].pops` and updates `pop_index`
- `add_facility(type, settlement_id, owner_id)` pushes to `settlement_states[settlement_id].facilities` and updates `facility_index`
- `get_pop(id)` needs a way to find which settlement a pop is in. Options:
  - Keep a global `HashMap<PopId, SettlementId>` reverse index (cheap, O(1))
  - Or require callers to provide the settlement (cleaner API, pushes settlement-awareness up)
- `get_facility(id)` — same choice

Decision: **keep a lightweight global reverse index** for the builder/accessor methods, since tests and setup code use `get_pop(id)` extensively without knowing the settlement. The hot tick path won't use it.

```rust
// In World:
pop_to_settlement: HashMap<PopId, SettlementId>,
facility_to_settlement: HashMap<FacilityId, SettlementId>,
```

**1d. Update accessor methods**

- `pops_at_settlement(sid)` → `&self.settlement_states[&sid].pops` (direct slice, no filter)
- `facilities_at_settlement(sid)` → `&self.settlement_states[&sid].facilities` (direct slice, no filter)
- `merchants_at_settlement(sid)` → derive from `settlement_states[&sid].facilities` owners (local scan, not global)
- `get_pop(id)` → use reverse index to find settlement, then index into vec
- `get_pop_mut(id)` → same

**1e. Compile and fix all call sites**

This will touch most of the codebase. The compiler will guide us — every `self.pops.get(&id)` becomes a lookup through the reverse index or a direct settlement-scoped access.

### Phase 2: Rewrite run_settlement_tick to take &mut SettlementState

**2a. Change the signature**

Current:
```rust
pub fn run_settlement_tick(
    tick: u64,
    settlement: SettlementId,
    pops: &mut [&mut Pop],
    merchants: &mut [&mut MerchantAgent],
    good_profiles: &[GoodProfile],
    needs: &HashMap<String, Need>,
    price_ema: &mut HashMap<GoodId, Price>,
    external_market: Option<&ExternalMarketConfig>,
    outside_flow_totals: Option<&mut OutsideFlowTotals>,
    subsistence_config: Option<&SubsistenceReservationConfig>,
    depth_multipliers: &HashMap<GoodId, f64>,
    subsistence_queue: Option<&[PopId]>,
) -> market::MultiMarketResult
```

New:
```rust
pub fn run_settlement_tick(
    tick: u64,
    state: &mut SettlementState,
    merchants: &mut [&mut MerchantAgent],   // still extracted (merchants are global)
    good_profiles: &[GoodProfile],
    needs: &HashMap<String, Need>,
    external_market: Option<&ExternalMarketConfig>,
    outside_flow_totals: Option<&mut OutsideFlowTotals>,
    subsistence_config: Option<&SubsistenceReservationConfig>,
) -> market::MultiMarketResult
```

`price_ema`, `depth_multipliers`, `subsistence_queue`, and `settlement_id` are all now inside `state`.

**2b. Eliminate the extract/reinsert loop in `World::run_tick`**

Current (world.rs:164-261): extracts pops, extracts merchants, calls run_settlement_tick, reinserts everything.

New:
```rust
for (sid, state) in &mut self.settlement_states {
    // Extract only merchants (they're global, need mutable refs)
    let merchant_ids: Vec<MerchantId> = state.facilities.iter()
        .map(|f| f.owner)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut extracted_merchants: Vec<(MerchantId, MerchantAgent)> = merchant_ids
        .iter()
        .filter_map(|id| self.merchants.remove(id).map(|m| (*id, m)))
        .collect();
    let mut merchant_refs: Vec<&mut MerchantAgent> =
        extracted_merchants.iter_mut().map(|(_, m)| m).collect();

    run_settlement_tick(
        self.tick,
        state,
        &mut merchant_refs,
        good_profiles,
        needs,
        self.external_market.as_ref(),
        Some(&mut self.outside_flow_totals),
        self.subsistence_reservation.as_ref(),
    );

    // Only merchants need reinsertion
    for (id, merchant) in extracted_merchants {
        self.merchants.insert(id, merchant);
    }
}
```

This can't iterate `&mut self.settlement_states` while also accessing `self.merchants` due to borrow splitting. Solutions:
- Extract all merchants once before the loop, reinsert after
- Or split World into separate fields and pass them independently

Preferred: **split the borrow** by restructuring the loop to take `&mut self.settlement_states` and `&mut self.merchants` as separate borrows. This can be done by destructuring or by extracting merchants into a local before the loop:

```rust
let mut merchants = std::mem::take(&mut self.merchants);
for (sid, state) in &mut self.settlement_states {
    // ... use &mut merchants directly, no extract/reinsert per settlement
    let merchant_ids = state.merchant_ids(); // derive from facility owners
    let mut merchant_refs: Vec<&mut MerchantAgent> = merchant_ids
        .iter()
        .filter_map(|id| merchants.get_mut(id))
        .collect();
    // ...
}
self.merchants = merchants;
```

This is one extract/reinsert for ALL merchants, not per-settlement. And it enables future parallelism since each settlement only touches its own merchants' stockpiles.

### Phase 3: Refactor the labor phase

The labor phase is the trickiest because it currently runs globally. It reads facilities from all settlements, generates bids, clears a unified labor market, then applies assignments back to pops across settlements.

**Key constraint**: `wage_ema` is global. Labor clearing uses it to prioritize skill markets. Assignments flow across all settlements.

**3a. Keep labor as a global phase, but read from SettlementStates**

Don't try to parallelize labor yet. Instead:
- Iterate `self.settlement_states` to collect facility bids (replacing the current `self.facilities.values()` loop)
- Iterate `self.settlement_states` to collect pop asks
- Clear globally (same as today)
- Apply assignments by looking up pops in their settlement's storage

```rust
// Bid generation: iterate facilities per settlement
for (sid, state) in &self.settlement_states {
    for facility in &state.facilities {
        // generate bids using state.price_ema, self.merchants, self.wage_ema
    }
}

// Ask generation: iterate pops per settlement
for (sid, state) in &self.settlement_states {
    for pop in &state.pops {
        // generate asks
    }
}

// Clearing: unchanged (global)
let result = clear_labor_markets(...);

// Apply assignments: use reverse index or settlement-aware lookup
for assignment in &result.assignments {
    let pop_id = PopId::new(assignment.worker_id);
    let sid = self.pop_to_settlement[&pop_id];
    let state = self.settlement_states.get_mut(&sid).unwrap();
    let pop = &mut state.pops[state.pop_index[&pop_id]];
    pop.employed_at = Some(assignment.facility_id);
    // ...
}
```

**3b. Update subsistence queue management**

`update_subsistence_queues()` currently iterates `self.settlements` and `self.pops`. Change to iterate `self.settlement_states`, accessing `state.pops` and `state.subsistence_queue` directly.

### Phase 4: Refactor mortality

Mortality is currently global but all operations are same-settlement. Restructure to process per-settlement:

```rust
for (sid, state) in &mut self.settlement_states {
    let outcomes: Vec<(usize, MortalityOutcome)> = state.pops
        .iter().enumerate()
        .map(|(i, pop)| {
            let food_sat = pop.need_satisfaction.get("food").copied().unwrap_or(0.0);
            (i, check_mortality(&mut rng, food_sat))
        })
        .collect();

    // Process deaths: remove from pops vec, update facility workers, distribute estate
    // All within this settlement's state — no cross-settlement access needed

    // Process growth: push new pops to state.pops
}
```

This becomes embarrassingly parallel once we handle the global `next_agent_id` counter (pre-allocate ID ranges per settlement before the loop).

**Compaction note**: Removing pops from the middle of a Vec requires either:
- `swap_remove` (O(1) but changes indices — need to update `pop_index`)
- Mark-and-compact at end of mortality phase (batch the removals)
- Use a `Vec<Option<Pop>>` with free list (avoids compaction but wastes cache space)

Recommendation: **swap_remove + update pop_index**. Deaths are rare relative to population size, so the index updates are cheap.

### Phase 5: Replace Pop's inner HashMaps with fixed-size arrays

This is the SoA payoff for the innermost hot loop (consumption, order generation).

**5a. Define MAX_GOODS**

```rust
pub const MAX_GOODS: usize = 16; // generous ceiling, currently ~10
```

**5b. Replace HashMap fields on Pop**

```rust
pub struct Pop {
    pub id: PopId,
    pub home_settlement: SettlementId,
    pub currency: f64,
    pub income_ema: f64,
    pub employed_at: Option<FacilityId>,
    pub skills: SmallVec<[SkillId; 4]>,  // or fixed array
    pub min_wage: Price,

    // Fixed-size arrays indexed by GoodId
    pub stocks: [f64; MAX_GOODS],
    pub desired_consumption_ema: [f64; MAX_GOODS],
    pub need_satisfaction: [f64; MAX_NEEDS],  // or keep HashMap if needs are dynamic
}
```

**5c. Update consumption, order generation, and market fill application**

These are mostly mechanical: replace `.get(&good).copied().unwrap_or(0.0)` with `[good as usize]`.

**5d. Same treatment for Stockpile, Facility.workers**

```rust
// Stockpile
pub struct Stockpile {
    pub goods: [f64; MAX_GOODS],
}

// Facility workers
pub struct Facility {
    pub workers: [u32; MAX_SKILLS],
    // ...
}
```

### Phase 6 (future): Rayon parallelism

With SettlementState owning all its data and merchants extracted once before the loop, the per-settlement tick becomes:

```rust
let mut merchants = std::mem::take(&mut self.merchants);
self.settlement_states.par_iter_mut().for_each(|(sid, state)| {
    // Each settlement gets its own merchant refs
    // Need to handle shared merchant access — see note below
});
self.merchants = merchants;
```

**Merchant contention**: If merchant M owns facilities in settlements A and B, both settlements need `&mut MerchantAgent` simultaneously. Solutions:
- Per-settlement merchant stockpile extraction (extract `HashMap<SettlementId, Stockpile>` into per-settlement owned data, merge back after)
- `Mutex<MerchantAgent>` (simple but adds lock overhead)
- Split merchant state: `MerchantSettlementState` (stockpile, production_ema for this settlement) lives in SettlementState, `MerchantGlobalState` (currency, facility_ids) stays in World

This is a phase 6 concern and doesn't need to be solved during the initial migration.

## Migration Order & Testing Strategy

Each phase should be a separate PR that keeps all tests passing:

1. **Phase 1+2** together (structural change + tick rewrite) — this is the big bang, unavoidable since the data structure change touches everything. Run full test suite including convergence tests to verify economic behavior is unchanged.

2. **Phase 3** (labor refactor) — should produce identical labor clearing results. Validate with instrumented tests comparing assignment counts and wages.

3. **Phase 4** (mortality refactor) — validate population trajectories match baseline.

4. **Phase 5** (fixed-size arrays) — purely mechanical, behavior identical. Good candidate for property-based testing (old vs new produce same results).

5. **Phase 6** (rayon) — add `rayon` parallelism, verify determinism with fixed seeds (may need to accept non-deterministic ordering for mortality within a settlement).

## Risk: Merchant State During Parallel Settlement Ticks

The one architectural decision that needs to be made before starting is how merchant state is partitioned. The cleanest long-term answer is:

```rust
// In SettlementState:
pub merchant_stockpiles: HashMap<MerchantId, Stockpile>,
pub merchant_production_ema: HashMap<MerchantId, HashMap<GoodId, f64>>,

// In World (global):
pub merchant_currency: HashMap<MerchantId, f64>,
pub merchant_facility_ids: HashMap<MerchantId, HashSet<FacilityId>>,
```

This splits merchant state along the settlement boundary. Per-settlement tick only touches the local stockpile. Wage payments deducting from `merchant_currency` happen in the labor phase (global, sequential). This avoids all contention in the parallel settlement phase.

Making this split in phase 1 (even before rayon) simplifies the whole migration because the merchant extract/reinsert problem goes away entirely.
