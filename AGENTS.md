# AGENTS.md

## Purpose

Working guide for coding agents in this repository.
For economic simulation tasks, use the docs below as the canonical context.

## Economic Docs: Read Order

MANDATORY STARTUP RULE:

1. At the start of every new conversation, read **all** documents in this section, in order.
2. At the start of any resumed conversation after compaction/context loss, re-read **all** documents in this section, in order.
3. Do not proceed with economic reasoning, implementation, or review until this full read pass is complete.

1. `sim-core/specs/current/README.md`
2. `sim-core/specs/current/TICK_STATE_SPEC.md`
3. `sim-core/specs/current/MARKETS_LABOR_SPEC.md`
4. `sim-core/specs/current/CONVERGENCE_INVARIANTS_SPEC.md`
5. `sim-core/specs/current/ASPIRATIONAL_DIRECTION_SPEC.md`
6. `economic_simulation_design_conversation_v2.md`
7. `sim-core/docs/pop_dynamics.md`

Historical context (not current source of truth):

1. `economic_simulation_design_conversation.md`

## Source-of-Truth Rules

1. Treat `sim-core/specs/current/*.md` as authoritative for current runtime behavior.
2. Treat `sim-core/specs/current/ASPIRATIONAL_DIRECTION_SPEC.md` and `economic_simulation_design_conversation_v2.md` as forward direction.
3. If docs conflict, prefer current runtime specs over legacy conversation notes.

## Key Direction (Embedded Summary)

1. External grain anchor should be soft and bounded, not a hard peg.
2. Large-network target topology is gateway-port based:
   - only designated port settlements connect directly to outside anchor liquidity.
   - non-port settlements access anchored goods through internal merchant trade.
3. Merchant-owned ships/caravans are intended to be the primary mechanism for inland propagation of goods and price signals.

## When Changing Economic Logic

POST-COMMIT DOC MAINTENANCE RULE:

1. After each commit, review the docs above for semantic drift.
2. Update docs only when behavior changed meaningfully or when code now contradicts existing wording.
3. Do not document trivial/internal refactors that do not change behavior or intent.
4. If nothing significant changed, make no doc edits and add no doc-related notes.

High-signal rule:

1. Keep docs concise and high signal because they are read frequently.
2. Prefer omitting low-value churn over documenting every minor implementation detail.

Implementation expectations:

1. Update affected current spec doc(s) in the same change when runtime behavior changes.
2. Add/update invariant or convergence tests for new behavior.
3. Call out whether the change modifies current behavior or only advances aspirational direction.
