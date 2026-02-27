# Determinism Contract

This simulation is intended to be deterministic under a fixed random seed and identical inputs.

## Rules

1. **Never iterate hash-backed collections directly in tie-sensitive paths.**
   - If iteration order can influence order IDs, tie-breaks, or floating-point accumulation,
     convert to a sorted list first.
2. **Use shared deterministic helpers.**
   - Use helpers in `sim-core/src/determinism.rs` to sort settlement IDs, merchant IDs,
     pop keys, and facility keys.
3. **Assign IDs only after deterministic ordering.**
   - Market order IDs and similar sequence IDs must be generated from sorted participants.
4. **Keep tie-break policies explicit.**
   - Where ties are possible (bids, asks, assignments), include stable secondary keys.
5. **Instrumentation IDs must be stable.**
   - When logging participant IDs, use stable numeric encodings (`AgentId::stable_u64`).

## Current tie-sensitive examples

- Settlement loop orchestration in `World::run_tick`
- Market participant extraction and order generation
- Labor assignment candidate ordering and reservation
- Mortality and subsistence queue ordering

Any new path that can affect simulation state should follow the same contract.
