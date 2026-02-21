use std::collections::HashMap;

use crate::types::{GoodId, PopId, Price};

/// Config for converting in-kind subsistence fallback into labor reservation asks.
#[derive(Debug, Clone)]
pub struct SubsistenceReservationConfig {
    /// Good used to value subsistence output (typically grain).
    pub grain_good: GoodId,
    /// Per-worker fallback output when subsistence labor is uncrowded.
    pub q_max: f64,
    /// Number of subsistence workers that can farm at full output (q_max).
    /// Beyond K, output drops linearly to zero at 2K.
    pub carrying_capacity: usize,
    /// Fallback price used when local grain EMA is missing.
    pub default_grain_price: Price,
    /// Fraction above break-even that subsistence pops demand before switching
    /// to formal employment. Reflects uncertainty about future grain prices.
    pub risk_premium: f64,
}

impl SubsistenceReservationConfig {
    pub fn new(grain_good: GoodId, q_max: f64, carrying_capacity: usize, default_grain_price: Price, risk_premium: f64) -> Self {
        Self { grain_good, q_max, carrying_capacity, default_grain_price, risk_premium }
    }
}

impl Default for SubsistenceReservationConfig {
    fn default() -> Self {
        Self {
            grain_good: 1,
            q_max: 1.5,
            carrying_capacity: 40,
            default_grain_price: 10.0,
            risk_premium: 0.10,
        }
    }
}

/// Piecewise-linear subsistence output per worker by rank.
///
/// - Ranks 1..=K: flat at q_max (uncrowded)
/// - Ranks K+1..2K: linear dropoff from q_max to 0
/// - Ranks > 2K: zero output
pub fn subsistence_output_per_worker(rank: usize, q_max: f64, carrying_capacity: usize) -> f64 {
    if rank == 0 {
        return 0.0;
    }
    let k = carrying_capacity.max(1) as f64;
    let r = rank as f64;
    if r <= k {
        q_max
    } else if r <= 2.0 * k {
        q_max * (2.0 * k - r) / k
    } else {
        0.0
    }
}

/// Compute ranked subsistence yields for a set of pops.
///
/// Pops are sorted by PopId ascending. Lower-ranked pops are treated as
/// more land-efficient and receive larger in-kind subsistence output.
pub fn ranked_subsistence_yields(
    pop_ids: &[PopId],
    q_max: f64,
    carrying_capacity: usize,
) -> Vec<(PopId, f64)> {
    let mut ids = pop_ids.to_vec();
    ids.sort_by_key(|id| id.0);

    ids.into_iter()
        .enumerate()
        .map(|(idx, pop_id)| {
            let rank = idx + 1;
            let qty = subsistence_output_per_worker(rank, q_max, carrying_capacity);
            (pop_id, qty)
        })
        .collect()
}

/// Compute subsistence yields ordered by a priority queue.
///
/// Pops in `queue` keep their position from the previous tick. Any unemployed
/// pops not in the queue are appended at the back, sorted by PopId as fallback.
pub fn ordered_subsistence_yields(
    queue: &[PopId],
    unemployed_ids: &[PopId],
    q_max: f64,
    carrying_capacity: usize,
) -> Vec<(PopId, f64)> {
    let unemployed_set: std::collections::HashSet<PopId> = unemployed_ids.iter().copied().collect();

    // Queue members that are still unemployed, in queue order
    let mut ordered: Vec<PopId> = queue
        .iter()
        .copied()
        .filter(|id| unemployed_set.contains(id))
        .collect();

    // Unemployed pops not in the queue go to the back, sorted by PopId
    let in_queue: std::collections::HashSet<PopId> = ordered.iter().copied().collect();
    let mut extras: Vec<PopId> = unemployed_ids
        .iter()
        .copied()
        .filter(|id| !in_queue.contains(id))
        .collect();
    extras.sort_by_key(|id| id.0);
    ordered.extend(extras);

    ordered
        .into_iter()
        .enumerate()
        .map(|(idx, pop_id)| {
            let rank = idx + 1;
            let qty = subsistence_output_per_worker(rank, q_max, carrying_capacity);
            (pop_id, qty)
        })
        .collect()
}

/// Build deterministic per-pop reservation wages from subsistence fallback.
///
/// Unemployed pops are ranked by queue position (priority queue) or PopId
/// ascending as fallback. Their reservation wages equal
/// `q(rank) * price * (1 + risk_premium)` — the subsistence output they'd get,
/// plus a premium reflecting price uncertainty.
///
/// Employed pops all receive a uniform reservation = `q(U+1) * price` — the
/// marginal subsistence output if one more worker joined the subsistence pool.
/// No risk premium for employed pops (they're already in the formal economy).
pub fn build_subsistence_reservation_ladder(
    employed_ids: &[PopId],
    unemployed_ids: &[PopId],
    grain_price_ref: Price,
    cfg: &SubsistenceReservationConfig,
    subsistence_queue: &[PopId],
) -> HashMap<PopId, Price> {
    let total = employed_ids.len() + unemployed_ids.len();
    let mut ladder = HashMap::with_capacity(total);

    // Unemployed: ranked reservation from queue-ordered subsistence yields + risk premium
    let yields = ordered_subsistence_yields(subsistence_queue, unemployed_ids, cfg.q_max, cfg.carrying_capacity);
    for (pop_id, qty) in &yields {
        ladder.insert(*pop_id, qty * grain_price_ref * (1.0 + cfg.risk_premium));
    }

    // Employed: marginal reservation = q(U+1) where U = number of unemployed (no premium)
    let marginal_rank = yields.len() + 1;
    let marginal_qty = subsistence_output_per_worker(marginal_rank, cfg.q_max, cfg.carrying_capacity);
    let marginal_reservation = marginal_qty * grain_price_ref;
    for &pop_id in employed_ids {
        ladder.insert(pop_id, marginal_reservation);
    }

    ladder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsistence_output_flat_below_capacity() {
        // Ranks 1..K should all produce q_max
        let q_max = 1.5;
        let k = 10;
        for rank in 1..=k {
            let output = subsistence_output_per_worker(rank, q_max, k);
            assert!(
                (output - q_max).abs() < 1e-9,
                "rank {rank} should produce q_max={q_max}, got {output}"
            );
        }
    }

    #[test]
    fn subsistence_output_drops_linearly_above_capacity() {
        let q_max = 1.5;
        let k = 10;
        // Rank K+1 should be just below q_max
        let at_k1 = subsistence_output_per_worker(k + 1, q_max, k);
        assert!(at_k1 < q_max);
        assert!(at_k1 > 0.0);

        // Rank 2K should be zero
        let at_2k = subsistence_output_per_worker(2 * k, q_max, k);
        assert!((at_2k).abs() < 1e-9, "rank 2K should produce 0, got {at_2k}");

        // Rank > 2K should be zero
        let beyond = subsistence_output_per_worker(2 * k + 5, q_max, k);
        assert!((beyond).abs() < 1e-9);

        // Monotonically decreasing in the dropoff zone
        for rank in (k + 1)..(2 * k) {
            let a = subsistence_output_per_worker(rank, q_max, k);
            let b = subsistence_output_per_worker(rank + 1, q_max, k);
            assert!(a > b, "output should decrease: rank {rank} ({a}) > rank {} ({b})", rank + 1);
        }
    }

    #[test]
    fn reservation_ladder_is_monotone_for_unemployed() {
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0, 0.10);
        // Use 15 unemployed — straddles the flat and dropoff zones
        let unemployed: Vec<PopId> = (1..=15).map(PopId::new).collect();
        let employed = vec![];
        let queue: Vec<PopId> = unemployed.clone();
        let ladder = build_subsistence_reservation_ladder(&employed, &unemployed, 10.0, &cfg, &queue);

        for i in 1..15u32 {
            assert!(
                ladder[&PopId::new(i)] >= ladder[&PopId::new(i + 1)],
                "ladder should be monotone: pop {} ({}) >= pop {} ({})",
                i, ladder[&PopId::new(i)], i + 1, ladder[&PopId::new(i + 1)]
            );
        }
    }

    #[test]
    fn employed_pops_get_marginal_reservation() {
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0, 0.10);
        let unemployed = vec![PopId::new(1), PopId::new(2), PopId::new(3)];
        let employed = vec![PopId::new(10), PopId::new(11)];
        let queue = unemployed.clone();
        let ladder = build_subsistence_reservation_ladder(&employed, &unemployed, 10.0, &cfg, &queue);

        // Employed reservation = q(U+1) * price where U=3, NO risk premium
        let expected = subsistence_output_per_worker(4, cfg.q_max, cfg.carrying_capacity) * 10.0;
        assert!((ladder[&PopId::new(10)] - expected).abs() < 1e-9);
        assert!((ladder[&PopId::new(11)] - expected).abs() < 1e-9);

        // Employed reservation should be < worst unemployed reservation (unemployed have premium)
        assert!(ladder[&PopId::new(10)] < ladder[&PopId::new(3)]);
    }

    #[test]
    fn worker_at_carrying_capacity_produces_q_max() {
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0, 0.10);
        let output = subsistence_output_per_worker(cfg.carrying_capacity, cfg.q_max, cfg.carrying_capacity);
        assert!(
            (output - cfg.q_max).abs() < 1e-9,
            "worker at rank=K should produce q_max, got {output}"
        );
    }

    #[test]
    fn ranked_subsistence_yields_give_later_pops_less() {
        // 3 pops, K=2 — first 2 get q_max, third gets less
        let pops = vec![PopId::new(11), PopId::new(9), PopId::new(10)];
        let yields = ranked_subsistence_yields(&pops, 1.0, 2);
        assert_eq!(yields.len(), 3);
        // Sorted by PopId ascending
        assert_eq!(yields[0].0, PopId::new(9));
        assert_eq!(yields[1].0, PopId::new(10));
        assert_eq!(yields[2].0, PopId::new(11));
        // First two at capacity → same output
        assert!((yields[0].1 - yields[1].1).abs() < 1e-9);
        // Third is beyond K → less output
        assert!(yields[1].1 > yields[2].1);
    }

    #[test]
    fn ordered_yields_respect_queue_order() {
        // Queue has pop 5, 3, 1 — they should get ranks 1, 2, 3 respectively
        let queue = vec![PopId::new(5), PopId::new(3), PopId::new(1)];
        let unemployed = vec![PopId::new(1), PopId::new(3), PopId::new(5)];
        let yields = ordered_subsistence_yields(&queue, &unemployed, 1.0, 2);
        assert_eq!(yields.len(), 3);
        assert_eq!(yields[0].0, PopId::new(5)); // rank 1
        assert_eq!(yields[1].0, PopId::new(3)); // rank 2
        assert_eq!(yields[2].0, PopId::new(1)); // rank 3
    }

    #[test]
    fn ordered_yields_appends_non_queue_pops_at_back() {
        // Queue has pop 5 only, but pops 3 and 7 are also unemployed
        let queue = vec![PopId::new(5)];
        let unemployed = vec![PopId::new(7), PopId::new(3), PopId::new(5)];
        let yields = ordered_subsistence_yields(&queue, &unemployed, 1.0, 10);
        assert_eq!(yields.len(), 3);
        assert_eq!(yields[0].0, PopId::new(5)); // queue member first
        assert_eq!(yields[1].0, PopId::new(3)); // non-queue sorted by id
        assert_eq!(yields[2].0, PopId::new(7));
    }

    #[test]
    fn risk_premium_increases_unemployed_reservation() {
        let cfg_no_premium = SubsistenceReservationConfig::new(1, 1.0, 10, 10.0, 0.0);
        let cfg_with_premium = SubsistenceReservationConfig::new(1, 1.0, 10, 10.0, 0.20);
        let unemployed = vec![PopId::new(1), PopId::new(2)];
        let employed = vec![];
        let queue = unemployed.clone();

        let ladder_no = build_subsistence_reservation_ladder(&employed, &unemployed, 5.0, &cfg_no_premium, &queue);
        let ladder_yes = build_subsistence_reservation_ladder(&employed, &unemployed, 5.0, &cfg_with_premium, &queue);

        // With 20% premium, unemployed reservation should be 20% higher
        let ratio = ladder_yes[&PopId::new(1)] / ladder_no[&PopId::new(1)];
        assert!((ratio - 1.20).abs() < 1e-9, "expected 1.20 ratio, got {ratio}");
    }
}
