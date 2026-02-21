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
}

impl SubsistenceReservationConfig {
    pub fn new(grain_good: GoodId, q_max: f64, carrying_capacity: usize, default_grain_price: Price) -> Self {
        Self { grain_good, q_max, carrying_capacity, default_grain_price }
    }

}

impl Default for SubsistenceReservationConfig {
    fn default() -> Self {
        Self {
            grain_good: 1,
            q_max: 1.5,
            carrying_capacity: 40,
            default_grain_price: 10.0,
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

/// Build deterministic per-pop reservation wages from subsistence fallback.
///
/// Unemployed pops are ranked by `PopId` ascending (1..U). Their reservation
/// wages equal `q(rank) * price` -- the subsistence output they'd get.
///
/// Employed pops all receive a uniform reservation = `q(U+1) * price` -- the
/// marginal subsistence output if one more worker joined the subsistence pool.
pub fn build_subsistence_reservation_ladder(
    employed_ids: &[PopId],
    unemployed_ids: &[PopId],
    grain_price_ref: Price,
    cfg: &SubsistenceReservationConfig,
) -> HashMap<PopId, Price> {
    let total = employed_ids.len() + unemployed_ids.len();
    let mut ladder = HashMap::with_capacity(total);

    // Unemployed: ranked reservation from actual subsistence yields
    for (pop_id, qty) in ranked_subsistence_yields(unemployed_ids, cfg.q_max, cfg.carrying_capacity) {
        ladder.insert(pop_id, qty * grain_price_ref);
    }

    // Employed: marginal reservation = q(U+1) where U = number of unemployed
    let marginal_rank = unemployed_ids.len() + 1;
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
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0);
        // Use 15 unemployed — straddles the flat and dropoff zones
        let unemployed: Vec<PopId> = (1..=15).map(PopId::new).collect();
        let employed = vec![];
        let ladder = build_subsistence_reservation_ladder(&employed, &unemployed, 10.0, &cfg);

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
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0);
        let unemployed = vec![PopId::new(1), PopId::new(2), PopId::new(3)];
        let employed = vec![PopId::new(10), PopId::new(11)];
        let ladder = build_subsistence_reservation_ladder(&employed, &unemployed, 10.0, &cfg);

        // Employed reservation = q(U+1) * price where U=3
        let expected = subsistence_output_per_worker(4, cfg.q_max, cfg.carrying_capacity) * 10.0;
        assert!((ladder[&PopId::new(10)] - expected).abs() < 1e-9);
        assert!((ladder[&PopId::new(11)] - expected).abs() < 1e-9);

        // Employed reservation should be <= worst unemployed reservation
        assert!(ladder[&PopId::new(10)] <= ladder[&PopId::new(3)]);
    }

    #[test]
    fn worker_at_carrying_capacity_produces_q_max() {
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0);
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
}
