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

/// Build deterministic per-pop reservation wages from subsistence fallback.
///
/// Pops are ranked by `PopId` ascending. Higher-ranked hires correspond to fewer
/// remaining subsistence workers and therefore higher reservation asks.
pub fn build_subsistence_reservation_ladder(
    pop_ids: &[PopId],
    grain_price_ref: Price,
    cfg: &SubsistenceReservationConfig,
) -> HashMap<PopId, Price> {
    let mut ids = pop_ids.to_vec();
    ids.sort_by_key(|id| id.0);

    let total = ids.len();
    let mut ladder = HashMap::with_capacity(total);

    for (idx, pop_id) in ids.into_iter().enumerate() {
        let hire_rank = idx + 1; // 1-indexed
        let subsistence_workers = total.saturating_sub(hire_rank).saturating_add(1);
        let q_sub = subsistence_output_per_worker(subsistence_workers, cfg.q_max, cfg.crowding_alpha);
        ladder.insert(pop_id, q_sub * grain_price_ref);
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
    fn reservation_ladder_is_monotone_by_hire_rank() {
        let cfg = SubsistenceReservationConfig {
            grain_good: 1,
            q_max: 2.0,
            crowding_alpha: 0.1,
            default_grain_price: 10.0,
        };
        let pops = vec![PopId::new(1), PopId::new(2), PopId::new(3), PopId::new(4)];
        let ladder = build_subsistence_reservation_ladder(&pops, 10.0, &cfg);

        assert!(ladder[&PopId::new(2)] >= ladder[&PopId::new(1)]);
        assert!(ladder[&PopId::new(3)] >= ladder[&PopId::new(2)]);
        assert!(ladder[&PopId::new(4)] >= ladder[&PopId::new(3)]);
    }
}
