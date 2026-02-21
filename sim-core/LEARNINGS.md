# Simulation Learnings

Hard-won insights from debugging sessions. Read at session start, prune if over 500 lines.

## Subsistence & Reservation Wages

- **Reservation wages must be employment-aware.** Ranking all pops (as if all 100 are farming) gives nonsensical reservations. Only unemployed pops farm; employed pops get marginal reservation `q(U+1) * price` where U = unemployed count. This makes reservation high when few are in subsistence (attractive alternative) and low when many are (crowded).

- **K (carrying capacity) should be proportional to population, not a small constant.** K=10 with 100 pops gives only 1% margin between MVP and reservation at typical unemployment levels, making equilibrium impossible. K=pop/2 works well — it says "the land could feed half the people."

- **Hard nominal min_wage kills recovery after deflation.** min_wage is price-independent, so when deflation pushes MVP below min_wage, hiring permanently stalls even though the subsistence reservation (which scales with price) would allow it. The subsistence reservation is the principled floor; min_wage should be 0 or negligible.

- **Subsistence output formula**: `q(rank) = q_max / (1 + alpha * (rank-1))` where `alpha = (q_max - 1.0) / (K - 1)`. Worker at rank K produces exactly 1.0 (= consumption need). q_max capped at 1.5.

## Price Dynamics & External Anchor

- **The external anchor bounds deflation, but only within its depth capacity.** Export tiers create buy orders at prices from `world_price * (1-band)` down to `world_price * (1-band) / (1+tier_step*8)`. With world_price=10, band=0.95 (90% transport + 5% spread), the export floor is [0.40, 0.50]. When local production roughly equals consumption, this floor works.

- **Trade depth responds to price deviation with lag (EMA).** When local prices are far from world price, the depth multiplier ramps up (models "word spreading" about arbitrage opportunities). Key params: alpha=0.1 (slow), elasticity=0.5, max_mult=10. Uses previous tick's price_ema as the signal.

- **Surplus economies stabilize below the export floor.** A breadbasket (2x production) with max 10x depth settles around price 0.35, below the theoretical floor [0.40, 0.50]. The price finds its own equilibrium where surplus matches amplified export capacity, not at the floor itself. This is economically correct — a remote settlement producing double its needs should have cheap grain.

- **More export capacity can amplify instability.** Raising MAX_MULT from 5→10 with deterministic wage grind went from 0 employment crashes to 1505/10k ticks. Deeper exports create wider price swings that amplify boom-bust cycles. The fix is damping the wage grind (stochastic lowering), not reducing export capacity.

- **Merchant stockpile delays price recovery.** When wages deflate, merchants accumulate grain (paying less for labor). When employment crashes, the merchant keeps selling from stock for many ticks, suppressing the price rise that should trigger rehiring. The merchant's grain mountain is a key variable to watch in any price spiral investigation.

- **Compute the theoretical price floor before running.** For anchor params (world_price, spread_bps, transport_bps, tier_step_bps), calculate: `export_price_tier0 = world_price * (1 - (spread_bps + transport_bps) / 10000)`. Then verify the simulation price stabilizes near this value.

## Bid Adjustment & Monopsony

- **Wage lowering must be stochastic, not deterministic.** A fixed 5%/tick grind is unrealistic and destabilizing — it relentlessly pushes wages below reservation, causing periodic employment crashes. Stochastic lowering (p=0.3 per tick) models employer inertia ("things are working, why change?") and dramatically cuts volatility (employment std 36→15, crashes 1505→29 per 10k ticks).

- **Bid adjustment should be asymmetric: urgent raise, cautious lower.** Unfilled slots = lost production right now → raise immediately (deterministic, +20%). Filled slots = everything's fine → lower cautiously (stochastic, 30% chance of -5%). This asymmetry is economically correct and stabilizing.

- **MVP = output_per_worker * output_price, not just output_price.** Previous bug had MVP = output_price, making every worker appear to produce the entire facility's output.

- **Monopsony detection: `can_attract_workers = total_workers > my_workers`.** Checks if there are any pops not employed by this merchant's facilities. When false, the raise path doesn't fire even with unfilled slots.

## Debugging Patterns

- **Always build a grain accounting table.** `production + subsistence - consumption - exports = residual`. This immediately shows whether the economy is balanced or where surplus/deficit accumulates. The merchant stockpile (residual accumulator) is the most important single diagnostic.

- **Watch for razor-thin margins.** When MVP and reservation are within a few percent, any bid adjustment noise causes employment oscillations or collapse. Compute `q(U+1) / production_rate` — if it's above 0.95, the margin is dangerously thin.

- **Export drain can worsen crises.** When the settlement is struggling, exports siphon grain away. Check external_flow DataFrame to see if this is happening during population declines.

- **The price_ema is self-reinforcing.** Pop buy orders use income_ema as budget, which tracks wages, which track clearing prices. Lower wages → less buying → lower prices → lower MVP → lower wages. The external anchor is the circuit breaker.

- **Use windowed statistics for equilibrium analysis, not sampled rows.** Split long runs (10k ticks) into 1000-tick windows. Compute mean/std of price, population, employment, merchant grain per window. Linear regression slope over the last 5000 ticks is the key equilibrium diagnostic — near-zero slope = stable. Also count crash events (employment=0) per window to detect limit cycles vs transients. A 200-tick run looked like a crash; at 10k ticks the same system was clearly stable.

- **Always test at multiple timescales.** A 500-tick run showed what looked like a non-equilibrium slow drift. At 10k ticks, the same parameters produced a clear stable equilibrium with near-zero trends. Short runs can be misleading due to startup transients and slow EMA convergence.
