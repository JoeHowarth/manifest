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
