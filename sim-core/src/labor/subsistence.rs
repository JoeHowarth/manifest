use std::collections::HashMap;

use crate::types::{GoodId, PopId, Price};

/// Config for converting in-kind subsistence fallback into labor reservation asks.
#[derive(Debug, Clone)]
pub struct SubsistenceReservationConfig {
    /// Good used to value subsistence output (typically grain).
    pub grain_good: GoodId,
    /// Per-worker fallback output when subsistence labor is uncrowded.
    pub q_max: f64,
    /// Diminishing factor: higher means faster crowding losses.
    pub crowding_alpha: f64,
    /// Fallback price used when local grain EMA is missing.
    pub default_grain_price: Price,
}

impl Default for SubsistenceReservationConfig {
    fn default() -> Self {
        Self {
            grain_good: 1,
            q_max: 2.0,
            crowding_alpha: 0.02,
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
/// Pops are ranked by `PopId` ascending. Higher-ranked hires correspond to fewer
/// remaining subsistence workers and therefore higher reservation asks.
pub fn build_subsistence_reservation_ladder(
    pop_ids: &[PopId],
    grain_price_ref: Price,
    cfg: &SubsistenceReservationConfig,
) -> HashMap<PopId, Price> {
    let mut ladder = HashMap::with_capacity(pop_ids.len());
    for (pop_id, qty) in ranked_subsistence_yields(pop_ids, cfg.q_max, cfg.crowding_alpha) {
        ladder.insert(pop_id, qty * grain_price_ref);
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
    fn reservation_ladder_is_monotone_by_efficiency_rank() {
        let cfg = SubsistenceReservationConfig {
            grain_good: 1,
            q_max: 2.0,
            crowding_alpha: 0.1,
            default_grain_price: 10.0,
        };
        let pops = vec![PopId::new(1), PopId::new(2), PopId::new(3), PopId::new(4)];
        let ladder = build_subsistence_reservation_ladder(&pops, 10.0, &cfg);

        assert!(ladder[&PopId::new(1)] >= ladder[&PopId::new(2)]);
        assert!(ladder[&PopId::new(2)] >= ladder[&PopId::new(3)]);
        assert!(ladder[&PopId::new(3)] >= ladder[&PopId::new(4)]);
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
}
