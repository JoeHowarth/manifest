use std::collections::HashMap;

use crate::types::{GoodId, PopId, Price};

/// Config for converting in-kind subsistence fallback into labor reservation asks.
#[derive(Debug, Clone)]
pub struct SubsistenceReservationConfig {
    /// Good used to value subsistence output (typically grain).
    pub grain_good: GoodId,
    /// Per-worker fallback output when subsistence labor is uncrowded (clamped <= 1.5).
    pub q_max: f64,
    /// Number of subsistence workers at which per-worker output equals exactly 1.0.
    pub carrying_capacity: usize,
    /// Fallback price used when local grain EMA is missing.
    pub default_grain_price: Price,
}

impl SubsistenceReservationConfig {
    pub fn new(grain_good: GoodId, q_max: f64, carrying_capacity: usize, default_grain_price: Price) -> Self {
        Self { grain_good, q_max: q_max.min(1.5), carrying_capacity, default_grain_price }
    }

    /// Derived crowding factor: ensures worker at rank=carrying_capacity produces exactly 1.0
    pub fn crowding_alpha(&self) -> f64 {
        (self.q_max - 1.0).max(0.0) / (self.carrying_capacity as f64 - 1.0).max(1.0)
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

/// Diminishing in-kind output per worker at subsistence labor count `workers`.
pub fn subsistence_output_per_worker(workers: usize, q_max: f64, crowding_alpha: f64) -> f64 {
    if workers == 0 {
        return 0.0;
    }
    let crowd = (workers as f64 - 1.0).max(0.0);
    q_max / (1.0 + crowding_alpha.max(0.0) * crowd)
}

/// Compute ranked subsistence yields for a set of pops.
///
/// Pops are sorted by PopId ascending. Lower-ranked pops are treated as
/// more land-efficient and receive larger in-kind subsistence output.
pub fn ranked_subsistence_yields(
    pop_ids: &[PopId],
    q_max: f64,
    crowding_alpha: f64,
) -> Vec<(PopId, f64)> {
    let mut ids = pop_ids.to_vec();
    ids.sort_by_key(|id| id.0);

    ids.into_iter()
        .enumerate()
        .map(|(idx, pop_id)| {
            let rank = idx + 1;
            let qty = subsistence_output_per_worker(rank, q_max, crowding_alpha);
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
    let alpha = cfg.crowding_alpha();
    let total = employed_ids.len() + unemployed_ids.len();
    let mut ladder = HashMap::with_capacity(total);

    // Unemployed: ranked reservation from actual subsistence yields
    for (pop_id, qty) in ranked_subsistence_yields(unemployed_ids, cfg.q_max, alpha) {
        ladder.insert(pop_id, qty * grain_price_ref);
    }

    // Employed: marginal reservation = q(U+1) where U = number of unemployed
    let marginal_rank = unemployed_ids.len() + 1;
    let marginal_qty = subsistence_output_per_worker(marginal_rank, cfg.q_max, alpha);
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
    fn subsistence_output_decreases_with_crowding() {
        let uncrowded = subsistence_output_per_worker(1, 2.0, 0.1);
        let crowded = subsistence_output_per_worker(20, 2.0, 0.1);
        assert!(uncrowded > crowded);
    }

    #[test]
    fn reservation_ladder_is_monotone_for_unemployed() {
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0);
        let unemployed = vec![PopId::new(1), PopId::new(2), PopId::new(3), PopId::new(4)];
        let employed = vec![];
        let ladder = build_subsistence_reservation_ladder(&employed, &unemployed, 10.0, &cfg);

        assert!(ladder[&PopId::new(1)] >= ladder[&PopId::new(2)]);
        assert!(ladder[&PopId::new(2)] >= ladder[&PopId::new(3)]);
        assert!(ladder[&PopId::new(3)] >= ladder[&PopId::new(4)]);
    }

    #[test]
    fn employed_pops_get_marginal_reservation() {
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0);
        let unemployed = vec![PopId::new(1), PopId::new(2), PopId::new(3)];
        let employed = vec![PopId::new(10), PopId::new(11)];
        let ladder = build_subsistence_reservation_ladder(&employed, &unemployed, 10.0, &cfg);

        // Employed reservation = q(U+1) * price where U=3
        let alpha = cfg.crowding_alpha();
        let expected = subsistence_output_per_worker(4, cfg.q_max, alpha) * 10.0;
        assert!((ladder[&PopId::new(10)] - expected).abs() < 1e-9);
        assert!((ladder[&PopId::new(11)] - expected).abs() < 1e-9);

        // Employed reservation should be <= worst unemployed reservation
        assert!(ladder[&PopId::new(10)] <= ladder[&PopId::new(3)]);
    }

    #[test]
    fn worker_at_carrying_capacity_produces_one() {
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0);
        let alpha = cfg.crowding_alpha();
        let output = subsistence_output_per_worker(cfg.carrying_capacity, cfg.q_max, alpha);
        assert!(
            (output - 1.0).abs() < 1e-9,
            "worker at rank=carrying_capacity should produce 1.0, got {output}"
        );
    }

    #[test]
    fn ranked_subsistence_yields_give_earlier_pops_more() {
        let pops = vec![PopId::new(11), PopId::new(9), PopId::new(10)];
        let yields = ranked_subsistence_yields(&pops, 1.0, 0.5);
        assert_eq!(yields.len(), 3);
        assert_eq!(yields[0].0, PopId::new(9));
        assert_eq!(yields[1].0, PopId::new(10));
        assert_eq!(yields[2].0, PopId::new(11));
        assert!(yields[0].1 > yields[1].1);
        assert!(yields[1].1 > yields[2].1);
    }

    #[test]
    fn crowding_alpha_derived_correctly() {
        let cfg = SubsistenceReservationConfig::new(1, 1.5, 10, 10.0);
        // alpha = (1.5 - 1.0) / (10 - 1) = 0.5 / 9
        let expected = 0.5 / 9.0;
        assert!((cfg.crowding_alpha() - expected).abs() < 1e-12);
    }
}
