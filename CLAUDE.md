# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Manifest is an economic simulation game set in the late 1700s. It models settlements, trade routes, production chains, and merchant organizations. The codebase is currently a Rust simulation core with an instrumentation layer — no frontend exists yet.

## Commands

```bash
# Run Rust tests
cd sim-core && cargo test

# Run a specific test file
cd sim-core && cargo test --test convergence

# Run with instrumentation
cd sim-core && cargo test --features instrument
```

## Architecture

### Simulation Core (`sim-core/`)

Modular Rust library. Key source files:

- **`src/lib.rs`**: Public API surface and module declarations
- **`src/world.rs`**: Global state (`World`), labor clearing, production phases
- **`src/tick.rs`**: Per-settlement tick engine (consumption, orders, market clearing, mortality)
- **`src/agents/`**: Pop and MerchantAgent definitions and state
- **`src/market/`**: Call auction clearing, demand/supply curves, order types
- **`src/labor/`**: Skill-based labor markets, MVP bidding, subsistence reservation
- **`src/production/`**: Recipes, facilities, multi-input execution
- **`src/consumption/`**: Greedy utility-maximization with budget constraints
- **`src/external/`**: Outside market anchors, import/export ladders with friction
- **`src/types/`**: Domain types (GoodId, SettlementId, UtilityCurve, etc.)

Key patterns:

- **HashMap IDs**: `SettlementId`, `PopId`, `MerchantId`, `FacilityId` as newtype-wrapped u32s
- **Tick order**: Labor → Production → Subsistence → Consumption → Order Generation → Market Clearing → Mortality
- **Call auction**: Volume-maximizing price discovery with proportional allocation and iterative budget relaxation
- **EMA smoothing**: Prices (α=0.3), wages (α=0.3), income (α=0.3), desired consumption (α=0.2)

### Instrumentation (`instrument/`)

Tracing subscriber that collects simulation events into column-oriented tables. Outputs Parquet files via polars for analysis in notebooks or duckdb.

### Tests (`sim-core/tests/`)

- **`invariants.rs`**: Conservation laws (currency, inventory, labor accounting)
- **`properties.rs`**: Property-based tests (no negative stocks/currency, bounded prices)
- **`convergence.rs`**: Parameter sweeps, calibration grids, snapshot regression
- **`convergence_baseline.rs`**: Multi-seed snapshot regression
- **`external_anchor.rs`**: Import/export anchor behavior
- **`carrying_capacity.rs`**: Population equilibrium under resource limits
- **`stability_investigation.rs`**: Instrumented DataFrame analysis of stability regimes

### Specs (`sim-core/specs/current/`)

- **`TICK_STATE_SPEC.md`**: World tick order, per-phase read/write effects
- **`MARKETS_LABOR_SPEC.md`**: Goods auction, demand curves, labor clearing
- **`CONVERGENCE_INVARIANTS_SPEC.md`**: Strict/weak convergence criteria, parameter sweeps

## Debugging Methodology: Instrumented DataFrames

When investigating simulation behavior (population traps, price spirals, equilibrium failures), **do not guess from code alone**. Use the instrumentation system to ask questions of the data.

### How to write an investigation test

1. Create an `#[ignore]` test in `stability_investigation.rs` that sets up the exact scenario you're investigating.
2. Wrap the tick loop with `ScopedRecorder::new("data/investigation", "descriptive_name")`.
3. Run with `cargo test --features instrument --test stability_investigation <test_name> -- --ignored --nocapture`.
4. Query the resulting DataFrames with polars to test specific hypotheses.

### Available instrument targets

These are the `target:` values in `tracing::info!` calls, each producing a DataFrame:

| Target | Key columns | What it tells you |
|---|---|---|
| `labor_bid` | tick, facility_id, max_wage, mvp, adaptive_bid | What facilities are offering and why |
| `labor_ask` | tick, pop_id, min_wage | Worker reservation wages |
| `assignment` | tick, pop_id, facility_id, wage | Who got hired at what price |
| `skill_outcome` | tick, facility_id, wanted, filled, profitable_unfilled | Bid adjuster inputs |
| `subsistence` | tick, pop_id, quantity | In-kind food output for unemployed |
| `mortality` | tick, pop_id, food_satisfaction, outcome, growth_prob | Who dies/grows and why |
| `order` | tick, agent_type, side, quantity, limit_price | Market order book |
| `fill` | tick, agent_type, side, quantity, price | Executed trades |
| `consumption` | tick, stock_before, desired, actual | Pop eating behavior |
| `production_io` | tick, direction, quantity | Facility input/output |
| `stock_flow` | tick, merchant_currency_after | Currency balances |
| `stock_flow_good` | tick, goods_after | Merchant inventory |
| `external_flow` | tick, flow, quantity | Import/export volumes |

### Investigation pattern

Structure investigations around **hypotheses**, not open-ended exploration:

1. **State what you expect** (e.g., "price should stabilize at the export floor ~0.45")
2. **Compute the theoretical prediction** from parameters before running
3. **Query a specific DataFrame** to confirm or refute
4. **Build grain accounting tables** (production + subsistence - consumption - exports = residual) to trace where resources go
5. **Sample at key ticks** to see trajectories, not just averages

Use `col_f64()` helper for extracting typed columns. Group by tick, aggregate, sort, and sample at specific tick indices. See existing investigation tests for patterns.

### When to use this approach

- A convergence test fails and the reason isn't obvious from the assertion
- You suspect a feedback loop (wage-price spiral, stockpile accumulation, etc.)
- You need to distinguish between multiple possible root causes
- You want to verify that a parameter change produces the expected economic outcome

## Learnings File

Maintain a running `sim-core/LEARNINGS.md` file with discoveries from debugging sessions, economic modeling insights, and parameter sensitivities. Update it whenever you learn something interesting about the simulation's behavior.

At the start of each new session, read `sim-core/LEARNINGS.md` and prune it:
- Remove entries that are no longer relevant (e.g., about code that has since been rewritten)
- Remove entries that aren't worth the space (obvious things, overly specific test details)
- If `wc -l` exceeds 500 lines, entries **must** be removed to stay under the limit
- Prefer keeping hard-won insights about economic dynamics and subtle interactions over implementation details
