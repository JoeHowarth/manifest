# Economic Simulation Design Conversation v2

## Overview

This v2 document updates the earlier design conversation to treat the **current implemented simulation** as the baseline, then describes how higher-level mechanics should build on top.

This is intentionally not a clean-room redesign. It is a migration-aware design that preserves working systems and extends them.

## Context Shift from v1 Conversation

The original conversation explored:

1. Organizations, hierarchies, contracts, and arbitration.
2. Trade/production/transport in a late premodern setting.
3. Information as a physical, delayed resource.
4. Avoiding a military-first game loop.

The key change in v2 is:

1. We now have a concrete simulation core with real behavior.
2. Convergence failures and invariants are visible and testable.
3. New mechanics must integrate with existing feedback loops, not bypass them.

## Baseline: Current Implemented Design

### 1) Atomic population unit is `Pop`

`Pop` is the core stateful economic actor:

1. Household stocks.
2. Currency.
3. Income EMA.
4. Need satisfaction.
5. Skills, reservation wage, employment state.

This is the primary substrate for labor supply, consumption demand, and demography.

### 2) Tick architecture is already modular and usable

Current world tick sequence:

1. Labor phase.
2. Production phase.
3. Settlement consumption/market phase.
4. Mortality/growth phase.

This phase ordering already supports extension with new systems (org contracts, transport assets, information propagation) without re-architecting the core loop.

### 3) Labor and goods markets are both live

1. Labor is currently bid/ask-based by skill, with adaptive facility wage bids.
2. Goods clear via constrained call auctions with budgets and inventories.
3. Pop market behavior is driven by stock-buffer and price EMA ladders.

This gives us working price formation and wage formation, even if not yet in final target form.

### 4) Subsistence exists in two useful forms

1. In-kind subsistence output injection for unemployed pops.
2. Subsistence-derived reservation ladder for labor asks.

This is exactly the right bridge to the long-term “outside option” labor floor design.

### 5) External anchoring already exists

Bounded outside import/export ladders can be enabled per settlement for anchored goods.

This provides:

1. Soft numeraire stabilization.
2. Explicit external account flows.
3. Finite depth and frictional bands (not a hard peg).

Current limitation:

1. Runtime does not yet enforce a network topology where only designated ports connect to the outside world.
2. Inland settlements can currently be configured with direct outside ladders, which is useful for testing but not the intended large-network economic structure.

### 6) Observability and validation are unusually strong

The project already has:

1. Invariant tests.
2. Convergence tests with strict/weak criteria.
3. Scenario sweeps and stress characterization.
4. Instrumentation capable of producing DataFrames and parquet.

This means design changes can be tested as system behavior changes, not just code correctness.

## Design Premise for v2

All higher-level game mechanics should be expressed as **constraints, flows, and control surfaces** on top of the current core, not as detached “meta systems.”

Concretely:

1. Organization politics should change who can buy/sell/hire/ship under what terms.
2. Information mechanics should change what decisions are possible, not directly set prices.
3. Transport mechanics should change delivery latency/cost/capacity, not spawn synthetic arbitrage rewards.
4. Governance/arbitration mechanics should influence relationship and market-access states, not override accounting.
5. External anchors should be reachable only through gateway-port settlements; non-port settlements should access anchored goods through internal merchant logistics.

## Higher-Level Mechanics Built on Current Base

### 1) Organization graph (control + contracts + arbitration)

Add an explicit graph over existing economic actors:

1. `control` links (hierarchy, partial authority, delegation).
2. `contract` links (supply, access, info-sharing, carriage, credit).
3. `arbiter` links (who enforces which contract classes).

This layer should influence runtime by:

1. Modifying permitted actions and order eligibility.
2. Applying fees, penalties, collateral locks, and access tiers.
3. Affecting trust and relationship states that drive future contract feasibility.

It should not directly mutate market prices or inventories outside normal flows.

### 2) Relationship tiers (not continuous diplomacy noise)

Use legible tier states:

1. Favored.
2. Neutral.
3. Disfavored.
4. Excluded.

Tier transitions are event-driven:

1. Contract breach.
2. Arbitration compliance/refusal.
3. Delivery reliability.
4. Repeated fair dealing.

This plugs into market behavior by:

1. Altering spreads, collateral requirements, priority, and access.
2. Controlling whether orders can be posted in specific venues.

### 3) Information as physically propagated market snapshots

Build on current price/market state by introducing delayed visibility:

1. Snapshot acquisition at settlements via presence/trade activity.
2. Snapshot transport by ships/caravans/couriers.
3. Optional contractual sharing pipelines.
4. Snapshot staleness with explicit timestamp aging.

Decision logic then consumes **known prices** rather than omniscient prices where appropriate.

Important: this should affect strategic quality and reaction speed, not alter clearing math itself.

### 4) Explicit transport assets and route programs

Introduce transport as first-class entities:

1. Ships/caravans with capacity, speed, upkeep, and risk profile.
2. Standing routes for efficiency and predictability.
3. One-off missions for opportunistic response.

Transport integrates with existing economy by:

1. Moving goods between settlement stock contexts.
2. Moving information snapshots.
3. Consuming labor and maintenance goods.
4. Exposing route congestion/chokepoints as endogenous market frictions.

In mature network mode, these assets are not optional flavor:

1. Merchant-owned ships/caravans are the primary mechanism by which inland/specialized settlements receive tradables from ports and export their surpluses.
2. This keeps relative prices and settlement specialization outcomes endogenous to route capacity, risk, and contract quality.

### 5) External access topology: gateway ports

The intended steady-state topology for large maps:

1. Only port-tagged settlements post outside import/export ladders against world grain reference.
2. Non-port settlements have no direct outside ladder participation.
3. Non-port access to anchored goods comes through inter-settlement merchant trade and transport routes.
4. Port throughput constraints become first-class controls on macro stability and inland price dispersion.

### 6) Production graph expansion with coupling

Expand from current simplified setups toward a compact but coupled good graph:

1. Staple food chain (grain and substitutes).
2. Fuel/material chain.
3. Tool/capital maintenance chain.
4. Transport build/repair chain.

The rule is to keep cross-links that produce real allocation tension:

1. Shared bottlenecks.
2. Substitutes for core needs.
3. Capital maintenance feedback on productivity.

### 7) Governance without conquest-first gameplay

Model force/coercion primarily as friction and downside:

1. Disruption of routes and confidence.
2. Insurance and risk-premium increases.
3. Market exclusion and collateral stress.

Cooperative/legal/contractual strategies should dominate in expected value under normal conditions.

## v2 Simulation Architecture (Incremental)

Keep existing modules as core, add layers rather than replacing:

1. `sim-core/src/world.rs` and tick orchestration remain central.
2. Market and labor modules remain the execution engine.
3. Add higher-level policy modules that produce constraints/orders:
   - org network state.
   - contract/arbitration state.
   - information state.
   - transport schedule state.

The integration contract:

1. High-level systems produce permitted actions and constraints.
2. Low-level systems execute flows and clearing.
3. Demography and stock-flow accounting remain downstream of actual outcomes.

## Gameplay Loop v2 (Player Perspective)

1. Build or restructure an organization network.
2. Negotiate contracts and enforce/contest arbitration.
3. Allocate transport capacity between cargo and information advantage.
4. Shape local production and labor conditions through wages, inputs, and access.
5. Manage exposure to external anchors and cross-settlement imbalances.
6. Adapt as relationships and information lags shift opportunities.

This yields “strategy through institutions and logistics,” not spreadsheet-only optimization.

## Convergence-Critical Design Rules

Any new subsystem must satisfy:

1. No bypass of budget/inventory constraints.
2. No hidden money or goods creation outside explicit modeled channels.
3. No direct manual override of prices or wages in normal operation.
4. All policy effects expressed through orders, constraints, costs, delays, or access.
5. New regimes covered by invariants and scenario sweeps.

## Staged Build Plan (Practical)

### Stage 0: Baseline lock and observability

1. Treat current convergence/invariant suite as baseline.
2. Expand per-tick stock-flow diagnostics and parquet outputs.
3. Freeze a set of scenario seeds for regression tracking.

### Stage 1: Org graph and contract/arbitration skeleton

1. Add relationship tiers and contract records.
2. Add breach and penalty mechanics.
3. Make penalties influence market access/collateral only.
4. Add tests for tier transitions and non-cascading sanction boundaries.

### Stage 2: Information transport layer

1. Implement snapshot objects and timestamps.
2. Implement acquisition by presence/trade volume.
3. Implement propagation via transport assets and contract sharing.
4. Gate AI decisions by known/stale information where configured.

### Stage 3: Explicit transport assets and route logic

1. Introduce ships/caravans as entities with route programs.
2. Move goods and snapshots through these assets.
3. Add congestion/capacity effects at chokepoints.
4. Add route-vs-mission behavior controls.
5. Enforce port-gated external access: outside ladders only on designated ports.
6. Require inland settlements to obtain anchor influence indirectly through merchant routes.

### Stage 4: Production graph enrichment

1. Add compact cross-linked goods and maintenance loops.
2. Ensure substitutions exist for key needs to avoid brittle single-good collapse.
3. Re-tune labor and demography controls under richer goods dynamics.

### Stage 5: Promotion of harder convergence gates

1. Promote selected stress scenarios to gating.
2. Tighten strict criteria gradually.
3. Require sustained survival and low-oscillation tails in imbalanced starts.

## Open Questions for v2

1. Do we keep ask-side labor permanently, or ship a configurable askless labor mode for A/B?
2. Should information asymmetry be player-only at first, or also constrain AI immediately?
3. How aggressive should arbitration penalties be before they destabilize commerce networks?
4. What minimum multi-good graph gives meaningful substitution without overwhelming tuning?
5. Which stress scenarios should become gating first after Stage 3?

## Success Criteria

v2 succeeds when:

1. The current economic core remains stable under added institutional layers.
2. Organization/information/transport mechanics create meaningful strategic differentiation.
3. Convergence behavior improves under imbalanced starts without hidden clamps.
4. The simulation remains legible through instrumentation and visual tooling.
5. The game feels like “operating a commercial polity,” not purely solving static equations.
