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
use slotmap::{SlotMap, new_key_type};

new_key_type! { pub struct PopKey; }
new_key_type! { pub struct FacilityKey; }

pub struct SettlementState {
    pub id: SettlementId,
    pub info: Settlement,               // name, position, resource_slots

    // Pops (dense generational arena — O(1) insert/remove/lookup, no manual index)
    pub pops: SlotMap<PopKey, Pop>,

    // Facilities (dense generational arena)
    pub facilities: SlotMap<FacilityKey, Facility>,

    // Per-settlement market & labor state
    pub price_ema: HashMap<GoodId, Price>,
    pub wage_ema: HashMap<SkillId, Price>,
    pub facility_bid_states: HashMap<FacilityKey, FacilityBidState>,
    pub subsistence_queue: Vec<PopKey>,
    pub depth_multipliers: HashMap<GoodId, f64>,
}
```

**Why SlotMap over Vec + HashMap index:** `slotmap` is already a dependency. It provides dense storage with O(1) insert, remove, and lookup. Generational keys prevent use-after-free bugs that would be easy to introduce with raw Vec indices during mortality's swap_remove. No manual index bookkeeping needed.

**Key type migration:** The current `PopId(u32)` / `FacilityId(u32)` newtypes become `PopKey` / `FacilityKey` (SlotMap generational keys) for internal storage. The old `PopId` can be kept as a stable external ID (for instrumentation/tracing) stored as a field on Pop, with a lightweight `HashMap<PopId, (SettlementId, PopKey)>` reverse index on World for the rare cases that need ID-based lookup from outside a settlement.

### World — global state + settlement container

```rust
pub struct World {
    pub tick: u64,
    pub settlement_states: HashMap<SettlementId, SettlementState>,
    pub routes: Vec<Route>,

    // Merchants remain global (they span settlements)
    pub merchants: HashMap<MerchantId, MerchantAgent>,

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
| `wage_ema` | `World.wage_ema[skill]` | `SettlementState.wage_ema[skill]` | Labor is per-settlement (pops can't commute between cities in the 1700s) |
| External market config | `World.external_market` | **Stays global** | Immutable config |
| Routes | `World.routes` | **Stays global** | Cross-settlement by definition |

## Migration Steps

### Phase 1: Introduce SettlementState, move pops and facilities

This is the structural change. Every subsequent phase is a refactor within this structure.

**1a. Create `SettlementState` struct**

Define `SettlementState` with `pops: SlotMap<PopKey, Pop>`, `facilities: SlotMap<FacilityKey, Facility>`. Keep all the same entity types — no field changes to `Pop` or `Facility` yet.

**1b. Change `World` to hold `HashMap<SettlementId, SettlementState>`**

Remove `World.pops`, `World.facilities`, `World.settlements`. Replace with `World.settlement_states`. Move `price_ema`, `wage_ema`, `facility_bid_states`, `subsistence_queues`, `trade_depth_multipliers` into `SettlementState`.

**1c. Update World builder methods**

- `add_settlement()` creates a `SettlementState` with empty SlotMaps
- `add_pop(settlement_id)` inserts into `settlement_states[settlement_id].pops` (SlotMap returns `PopKey`)
- `add_facility(type, settlement_id, owner_id)` inserts into `settlement_states[settlement_id].facilities`
- `get_pop(id)` needs a way to find which settlement a pop is in — keep a lightweight reverse index:

```rust
// In World:
pop_to_settlement: HashMap<PopKey, SettlementId>,
facility_to_settlement: HashMap<FacilityKey, SettlementId>,
```

Tests and setup code use `get_pop(id)` extensively without knowing the settlement. The hot tick path won't use this reverse index — it iterates the SlotMap directly.

**1d. Update accessor methods**

- `pops_at_settlement(sid)` → `&self.settlement_states[&sid].pops` (iterate SlotMap directly)
- `facilities_at_settlement(sid)` → `&self.settlement_states[&sid].facilities`
- `merchants_at_settlement(sid)` → derive from `settlement_states[&sid].facilities` owners (local scan, not global)
- `get_pop(key)` → use reverse index to find settlement, then `state.pops[key]`
- `get_pop_mut(key)` → same

**1e. Compile and fix all call sites**

This will touch most of the codebase. The compiler will guide us — every `self.pops.get(&id)` becomes either a direct SlotMap access within a settlement context, or a reverse-index lookup for global access.

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

### Phase 3: Make labor per-settlement

The labor phase is currently global — it pools ALL facilities' bids and ALL pops' asks into one `clear_labor_markets` call. But there's no fundamental reason for this:
- Pops are settlement-bound (`home_settlement` is fixed, they can't commute)
- Facilities are settlement-bound (`settlement: SettlementId` is fixed)
- A London pop being assigned to a Paris facility is a bug, not a feature
- `clear_labor_markets` has zero settlement filtering — it just happens to work with one settlement

The current global `wage_ema: HashMap<SkillId, Price>` reinforces the accident — one wage per skill across all settlements. In reality, 1780s wages varied enormously by location.

**3a. Move wage_ema into SettlementState**

Each settlement gets its own `wage_ema: HashMap<SkillId, Price>`. Initialize from the global value during migration.

**3b. Run labor clearing per-settlement**

The labor phase becomes a method on SettlementState (or a free function taking `&mut SettlementState`):

```rust
for (sid, state) in &mut self.settlement_states {
    // Bids: only this settlement's facilities
    let bids = state.facilities.iter()
        .flat_map(|f| generate_facility_bids(f, state.price_ema, state.wage_ema, ...))
        .collect();

    // Asks: only this settlement's pops
    let asks = state.pops.iter()
        .flat_map(|p| generate_pop_asks(p, ...))
        .collect();

    // Clear this settlement's labor market independently
    let result = clear_labor_markets(&skills, &bids, &asks, &state.wage_ema, &budgets);

    // Update this settlement's wage_ema
    update_wage_emas(&mut state.wage_ema, &result);

    // Apply assignments — all pops and facilities are local, no reverse index needed
    for assignment in &result.assignments {
        let pop = state.pops.get_mut(pop_key);
        pop.employed_at = Some(assignment.facility_id);
        // ...
    }
}
```

**3c. The "no double hire" concern disappears**

Since pops only appear in their own settlement's ask list, the global `filled_workers: HashSet<u32>` dedup in `clear_labor_markets` still works but is now purely local. No pop can be double-hired across settlements.

**3d. Update subsistence queue management**

`update_subsistence_queues()` currently iterates `self.settlements` and `self.pops`. Change to iterate `self.settlement_states`, accessing `state.pops` and `state.subsistence_queue` directly.

**3e. Future: cross-settlement wage information diffusion**

If desired later, merchants operating in multiple settlements could carry wage signals between them as a separate post-tick diffusion step. This is a feature to add intentionally, not an accident of flat data structures.

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

Recommendation: **use `SlotMap`** (already a dependency). Generational keys handle removal correctly with no manual index bookkeeping, and internal storage is dense. See the note on SlotMap below.

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

1. **Phase 1+2+3** together (structural change + tick rewrite + per-settlement labor) — this is the big bang, unavoidable since the data structure change touches everything. With labor also going per-settlement, there's no reason to keep a global labor phase as an intermediate step. Run full test suite including convergence tests to verify economic behavior is unchanged. Note: per-settlement labor will change convergence behavior slightly (settlements no longer share a wage signal), so baselines may need updating.

2. **Phase 4** (mortality per-settlement) — validate population trajectories match baseline.

3. **Phase 5** (fixed-size arrays) — purely mechanical, behavior identical. Good candidate for property-based testing (old vs new produce same results).

4. **Phase 6** (rayon) — add `rayon` parallelism, verify determinism with fixed seeds (may need to accept non-deterministic ordering for mortality within a settlement).

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
