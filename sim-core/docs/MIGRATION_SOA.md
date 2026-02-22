# Migration Plan: Per-Settlement SoA Architecture

## Goal

Move all settlement-local state (pops, facilities, prices, wages, bid states) into a `SettlementState` struct. This eliminates the extract/reinsert ceremony, makes settlement-scoped iteration O(local) instead of O(global), and enables future per-settlement parallelism.

## Current Pain Points

1. **Extract/reinsert ceremony** (world.rs:188-255) — pops and merchants are removed from global HashMaps, processed, then reinserted every settlement every tick
2. **Linear scans for settlement membership** — `facilities_at_settlement()` scans all facilities; `merchants_at_settlement()` does the same then deduplicates
3. **Global labor phase is wrong** — `clear_labor_markets` has zero settlement filtering; a London pop can be assigned to a Paris facility. Only works by accident with one settlement.
4. **Borrow checker friction** — `run_labor_phase` iterates facilities while looking up merchants, price_ema, and wage_ema from `&mut self`

## Target Data Structures

### SettlementState

```rust
use slotmap::{SlotMap, new_key_type};

new_key_type! { pub struct PopKey; }
new_key_type! { pub struct FacilityKey; }

pub struct SettlementState {
    pub id: SettlementId,
    pub info: Settlement,               // name, position, resource_slots

    // Pops (dense generational arena)
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

**Why SlotMap:** Already a dependency. Dense storage with O(1) insert/remove/lookup. Generational keys prevent use-after-free bugs during mortality removals. No manual index bookkeeping.

**Key type migration:** `PopId(u32)` / `FacilityId(u32)` become `PopKey` / `FacilityKey` internally. Keep the old `PopId` as a stable external ID (field on Pop, used for instrumentation/tracing). A lightweight `HashMap<PopId, (SettlementId, PopKey)>` reverse index on World handles the rare global-lookup case (tests, cross-settlement references in accounting).

### World

```rust
pub struct World {
    pub tick: u64,
    pub settlement_states: HashMap<SettlementId, SettlementState>,
    pub routes: Vec<Route>,

    // Merchants remain global (span settlements via stockpiles)
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

    // Reverse indices (for global lookups from tests/accounting)
    pop_to_settlement: HashMap<PopKey, SettlementId>,
    facility_to_settlement: HashMap<FacilityKey, SettlementId>,
}
```

### What moves vs. stays

| Data | Current | New | Why |
|------|---------|-----|-----|
| Pops | `World.pops` | `SettlementState.pops` | Settlement-bound, never move |
| Facilities | `World.facilities` | `SettlementState.facilities` | Settlement-bound |
| `price_ema` | `World.price_ema[(sid,gid)]` | `SettlementState.price_ema[gid]` | Only accessed per-settlement |
| `wage_ema` | `World.wage_ema[skill]` | `SettlementState.wage_ema[skill]` | Labor is per-settlement (pops can't commute) |
| `facility_bid_states` | `World.facility_bid_states` | `SettlementState.facility_bid_states` | Per-facility → per-settlement |
| `subsistence_queues` | `World.subsistence_queues[sid]` | `SettlementState.subsistence_queue` | Already per-settlement |
| `depth_multipliers` | `World.trade_depth_multipliers[(sid,gid)]` | `SettlementState.depth_multipliers[gid]` | Already per-settlement-keyed |
| Merchants | `World.merchants` | **Stays global** | Span settlements via stockpiles |
| External market config | `World.external_market` | **Stays global** | Immutable config |
| Routes | `World.routes` | **Stays global** | Cross-settlement by definition |

## New Tick Structure

Current `run_tick` has three scopes: global labor, global production, per-settlement market tick, global mortality. After migration, everything is per-settlement:

```
run_tick:
    tick += 1
    capture pre-tick snapshot (accounting)

    let mut merchants = std::mem::take(&mut self.merchants);

    for (sid, state) in &mut self.settlement_states {
        // Extract merchant refs for this settlement
        let merchant_ids = state.local_merchant_ids();
        let mut merchant_refs: Vec<&mut MerchantAgent> = merchant_ids
            .iter()
            .filter_map(|id| merchants.get_mut(id))
            .collect();

        // All phases run per-settlement:
        state.run_labor(&mut merchant_refs, recipes, &config);
        state.run_production(&mut merchant_refs, recipes);
        state.run_subsistence(&config);
        state.run_consumption(good_profiles, needs);
        state.run_market_clearing(&mut merchant_refs, good_profiles, &config, &mut outside_flow_totals);
        state.run_price_ema_update(good_profiles, &config);
        state.run_mortality(tick, mortality_grace_ticks, &mut next_agent_id);
    }

    self.merchants = merchants;

    capture post-tick snapshot (accounting)
```

**Borrow splitting:** `std::mem::take` moves merchants out of World for the duration of the loop. This lets us iterate `&mut self.settlement_states` while mutating merchants independently. One take/restore per tick, not per settlement.

**Merchant ref safety:** Within a single settlement tick, we only touch each merchant's stockpile at *this* settlement. Two settlements could share a merchant, but since this is sequential, there's no conflict. Under future rayon parallelism, the stockpile split (see below) resolves this.

## Rewrite Checklist

This is one atomic change — every item must be done together because removing `World.pops` breaks all downstream code.

### 1. Define SettlementState, update World struct

- Create `SettlementState` with SlotMaps, price_ema, wage_ema, facility_bid_states, subsistence_queue, depth_multipliers
- Replace `World.pops`, `World.facilities`, `World.settlements`, `World.price_ema`, `World.wage_ema`, `World.facility_bid_states`, `World.subsistence_queues`, `World.trade_depth_multipliers` with `World.settlement_states`
- Add reverse index maps

### 2. Update World builder/accessor methods

- `add_settlement()` → creates `SettlementState`
- `add_pop(sid)` → `settlement_states[sid].pops.insert(...)`, updates reverse index
- `add_facility(type, sid, owner)` → `settlement_states[sid].facilities.insert(...)`, updates reverse index
- `get_pop(key)` → reverse index lookup → `state.pops[key]`
- `get_pop_mut(key)` → same, mutable
- `pops_at_settlement(sid)` → `&settlement_states[sid].pops` (direct, no scan)
- `facilities_at_settlement(sid)` → `&settlement_states[sid].facilities` (direct)
- `merchants_at_settlement(sid)` → derive from local facilities' owners

### 3. Rewrite run_labor_phase → per-settlement

Current: global `run_labor_phase(&mut self)` iterates all facilities, all pops, clears one global labor market.

New: `SettlementState::run_labor(...)` or free function taking `&mut SettlementState`.

Per settlement:
- Generate bids from `state.facilities` using `state.price_ema`, `state.wage_ema`
- Generate asks from `state.pops`
- `clear_labor_markets(...)` with this settlement's bids/asks/wage_ema
- `update_wage_emas(&mut state.wage_ema, ...)`
- Apply assignments to `state.pops` and `state.facilities` (all local, no reverse index needed)
- Update `state.facility_bid_states`
- Update `state.subsistence_queue`

The `filled_workers: HashSet` in `clear_labor_markets` still works but is now purely local — no cross-settlement double-hire possible since pops only appear in their own settlement's ask list.

### 4. Rewrite run_production_phase → per-settlement

Current: global `run_production_phase(&mut self)` iterates `self.facilities.keys()`, looks up `self.settlements` for quality, looks up `self.merchants` for stockpiles.

New: runs within per-settlement loop.

Per settlement:
- Iterate `state.facilities`
- Quality multiplier from `state.info.get_facility_slot()`
- Merchant stockpile from `merchant_refs` (already extracted)
- `allocate_recipes(...)` and `execute_production(...)`
- Update merchant `production_ema`

### 5. Rewrite run_settlement_tick → methods on SettlementState

Current signature has 12 parameters. Most become fields of `SettlementState`.

The current `run_settlement_tick` in tick.rs covers: subsistence, consumption, order generation, market clearing, fill application, price EMA update. These become methods (or stay as one method) on `SettlementState`, taking only:
- `tick: u64`
- `merchants: &mut [&mut MerchantAgent]`
- `good_profiles`, `needs` (global config refs)
- `external_market` config ref
- `&mut OutsideFlowTotals` (global accumulator)
- `subsistence_config` ref

`price_ema`, `depth_multipliers`, `subsistence_queue`, `settlement_id` are all accessed via `&mut self`.

### 6. Rewrite run_mortality_phase → per-settlement

Current: global, iterates `self.pops`, removes dead, adds children, distributes estate.

New: runs within per-settlement loop.

Per settlement:
- Iterate `state.pops`, compute outcomes
- Deaths: `state.pops.remove(key)`, update `state.facilities` workers, distribute estate to other pops in `state.pops` (all local)
- Growth: `state.pops.insert(child)`, update reverse index
- ID allocation: pass `&mut next_agent_id` into the per-settlement mortality call, or pre-allocate ID ranges

SlotMap handles removal cleanly — no swap_remove or compaction needed.

### 7. Update accounting (capture_world_flow_snapshot)

Current: iterates `world.pops.values()` and `world.merchants.values()`.

New: iterates `world.settlement_states.values().flat_map(|s| s.pops.values())` for pop currency/stocks. Merchant iteration is unchanged.

### 8. Update instrumentation

Tracing calls use `pop.id.0`, `facility_id.0`, `settlement_id.0` as integers. Keep `PopId` as a field on `Pop` so instrument target columns stay the same. No parquet schema changes needed.

### 9. Delete legacy code

- Remove `run_market_tick` (deprecated wrapper in tick.rs:601-630)
- Remove the extract/reinsert loop from `World::run_tick`
- Remove `Settlement.pop_ids: Vec<PopId>` (pops now live in SettlementState's SlotMap)

### 10. Fix tests

Tests use `world.add_pop(london)`, `world.get_pop(id)`, `world.get_pop_mut(id)`. These keep working via the updated builder/accessor methods. Convergence baselines may shift slightly because per-settlement labor changes wage dynamics (no shared wage signal across settlements). Update baselines after verifying behavior is economically reasonable.

## Future Work (not part of this migration)

### Rayon parallelism

With `SettlementState` owning all local data and merchants extracted via `mem::take`, the loop becomes `par_iter_mut`. One remaining problem: if merchant M owns facilities in settlements A and B, both need `&mut MerchantAgent` simultaneously.

Solutions:
- Split merchant state: `MerchantSettlementState` (stockpile, production_ema) lives in `SettlementState`; `MerchantGlobalState` (currency, facility_ids) stays in World. Settlement tick only touches local stockpile.
- Or `Mutex<MerchantAgent>` (simple but adds lock overhead)
- Or extract per-settlement stockpile slices before the parallel loop, merge after

### `outside_flow_totals` under parallelism

Currently passed as `&mut OutsideFlowTotals` into each settlement tick. Under rayon: either per-settlement accumulation + merge, or `Mutex`.

### Cross-settlement wage diffusion

Per-settlement labor means settlements don't share wage information. If desired, merchants operating in multiple settlements could carry wage signals as a post-tick diffusion step. This should be an intentional feature, not an accident of flat data structures.

### Fixed-size arrays for Pop fields

Replace `HashMap<GoodId, f64>` on Pop (stocks, desired_consumption_ema) with `[f64; MAX_GOODS]`. Purely a cache-locality optimization, independent of the structural migration. Adds a hard `MAX_GOODS` ceiling. Defer until profiling shows it matters.
