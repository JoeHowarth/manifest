# Current Specs (Implementation Truth)

These docs capture the behavior that is actually implemented in `sim-core/src` today.

Use this directory as the authoritative spec for:

1. Tick order and state transitions.
2. Pop/labor/goods market mechanics.
3. Mortality/growth and stabilization controls.
4. Convergence criteria and invariants used in tests.

## Documents

1. `TICK_STATE_SPEC.md`
   - World tick order.
   - Per-phase read/write effects.
   - Canonical state transition map.
2. `MARKETS_LABOR_SPEC.md`
   - Goods auction and demand/supply ladders.
   - Labor market clearing and adaptive bid behavior.
   - Subsistence reservation and external anchor interactions.
3. `CONVERGENCE_INVARIANTS_SPEC.md`
   - Strict/weak convergence criteria in tests.
   - Parameter sweeps and stress characterization.
   - Enforced invariants.
4. `ASPIRATIONAL_DIRECTION_SPEC.md`
   - Target architecture and feedback-loop goals.
   - Incremental migration path from current runtime.
   - Exit criteria for a convergence-ready core.

## Scope Notes

- These specs are descriptive, not aspirational.
- If a statement here conflicts with `sim-core/src`, the code is correct and this doc should be updated.
- Legacy design material remains in `sim-core/specs/v1` and `sim-core/specs/v2` for context only.
- Detailed pop-focused flow is also documented in `sim-core/docs/pop_dynamics.md`.
- The single directional doc in this folder is `ASPIRATIONAL_DIRECTION_SPEC.md`.
