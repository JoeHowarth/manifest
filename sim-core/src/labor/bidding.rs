//! Facility labor bidding logic.
//!
//! Facilities adjust their wage bids based on whether they filled slots last tick:
//! - Unfilled profitable slots → raise bid (up to marginal MVP)
//! - Filled + excess workers globally → lower bid
//! - Filled + tight market → hold steady
//!
//! Key insight: The adaptive bid is our estimate of the market clearing wage.
//! Each slot's actual bid is `min(adaptive_bid, slot_mvp)`. We only raise when
//! there are unfilled slots that are still profitable (MVP > adaptive_bid).

use std::collections::HashMap;

use super::skills::SkillId;
use crate::types::Price;

/// Outcome of a skill's labor market participation for one tick.
/// Used to inform bid adjustment for the next tick.
#[derive(Debug, Clone, Default)]
pub struct SkillOutcome {
    /// Number of slots that were filled
    pub filled: u32,
    /// Number of unfilled slots where MVP > adaptive_bid (worth raising for)
    pub profitable_unfilled: u32,
    /// MVP of the marginal profitable unfilled slot (cap for raising)
    /// This is the lowest MVP among unfilled slots that are still profitable.
    pub marginal_profitable_mvp: Option<Price>,
}

/// Tracks a facility's current wage bid for each skill.
/// Bids adjust over time based on fill rate.
#[derive(Debug, Clone, Default)]
pub struct FacilityBidState {
    /// Current adaptive bid per skill (our estimate of market rate)
    pub bids: HashMap<SkillId, Price>,
    /// Last tick's outcome per skill
    pub last_outcome: HashMap<SkillId, SkillOutcome>,
}

impl FacilityBidState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get current adaptive bid for a skill, or initialize from wage EMA
    pub fn get_bid(&self, skill: SkillId, wage_ema: Price) -> Price {
        self.bids.get(&skill).copied().unwrap_or(wage_ema)
    }

    /// Record the outcome of labor market clearing for a skill.
    ///
    /// - `filled`: Number of slots that were filled
    /// - `profitable_unfilled`: Unfilled slots where MVP > adaptive_bid
    /// - `marginal_profitable_mvp`: MVP of the lowest profitable unfilled slot
    pub fn record_outcome(
        &mut self,
        skill: SkillId,
        filled: u32,
        profitable_unfilled: u32,
        marginal_profitable_mvp: Option<Price>,
    ) {
        self.last_outcome.insert(
            skill,
            SkillOutcome {
                filled,
                profitable_unfilled,
                marginal_profitable_mvp,
            },
        );
    }

    /// Adjust bid for next tick based on last tick's outcome.
    ///
    /// - `wage_ema`: Current market wage EMA (used as floor/reference)
    /// - `global_excess_workers`: True if total workers > total jobs globally
    pub fn adjust_bid(&mut self, skill: SkillId, wage_ema: Price, global_excess_workers: bool) {
        let current_bid = self.get_bid(skill, wage_ema);
        let outcome = self.last_outcome.get(&skill).cloned().unwrap_or_default();

        let new_bid = if outcome.profitable_unfilled > 0 {
            // Unfilled profitable slots: raise bid by 20% (up to marginal MVP)
            let cap = outcome.marginal_profitable_mvp.unwrap_or(current_bid);
            (current_bid * 1.2).min(cap)
        } else if outcome.filled > 0 && global_excess_workers {
            // Filled + excess workers: lower bid by 5% (floor at some minimum)
            let floor = wage_ema * 0.5; // Don't go below half of market rate
            (current_bid * 0.95).max(floor)
        } else {
            // Filled + tight market: hold steady
            current_bid
        };

        self.bids.insert(skill, new_bid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: u32) -> SkillId {
        SkillId(id)
    }

    #[test]
    fn profitable_unfilled_slots_raise_bid() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        // Start at adaptive bid = 10
        state.bids.insert(laborer, 10.0);

        // Filled 1 slot, but 2 profitable unfilled slots remain (MVP > bid)
        // Marginal profitable MVP = 50 (the cap for raising)
        state.record_outcome(laborer, 1, 2, Some(50.0));

        // Adjust
        state.adjust_bid(laborer, 10.0, false);

        // Should raise by 20%: 10 * 1.2 = 12
        assert_eq!(state.bids.get(&laborer), Some(&12.0));
    }

    #[test]
    fn unprofitable_unfilled_slots_dont_raise_bid() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        // Adaptive bid = 50
        state.bids.insert(laborer, 50.0);

        // Filled 2 slots, 1 unfilled but it's unprofitable (MVP < bid)
        // So profitable_unfilled = 0
        state.record_outcome(laborer, 2, 0, None);

        // Adjust (no excess workers, so should hold steady)
        state.adjust_bid(laborer, 10.0, false);

        // Should hold steady since no profitable unfilled slots
        assert_eq!(state.bids.get(&laborer), Some(&50.0));
    }

    #[test]
    fn filled_with_excess_workers_lowers_bid() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 20.0);
        // All slots filled, no profitable unfilled
        state.record_outcome(laborer, 3, 0, None);

        // Adjust with excess workers globally
        state.adjust_bid(laborer, 10.0, true);

        // Should lower by 5%: 20 * 0.95 = 19
        assert_eq!(state.bids.get(&laborer), Some(&19.0));
    }

    #[test]
    fn filled_tight_market_holds_steady() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 15.0);
        // All slots filled
        state.record_outcome(laborer, 2, 0, None);

        // Adjust with tight market (no excess workers)
        state.adjust_bid(laborer, 10.0, false);

        // Should hold steady
        assert_eq!(state.bids.get(&laborer), Some(&15.0));
    }

    #[test]
    fn bid_capped_at_marginal_mvp() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 90.0);
        // No slots filled, 3 profitable unfilled with marginal MVP = 100
        state.record_outcome(laborer, 0, 3, Some(100.0));

        // Adjust
        state.adjust_bid(laborer, 10.0, false);

        // 90 * 1.2 = 108, but capped at marginal MVP = 100
        assert_eq!(state.bids.get(&laborer), Some(&100.0));
    }

    #[test]
    fn bid_has_floor() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 6.0);
        // All filled, no profitable unfilled
        state.record_outcome(laborer, 3, 0, None);

        // Keep lowering with excess workers
        for _ in 0..20 {
            state.adjust_bid(laborer, 10.0, true);
        }

        // Should hit floor at wage_ema * 0.5 = 5.0
        let bid = state.bids.get(&laborer).unwrap();
        assert!(*bid >= 5.0, "bid {} should be >= floor 5.0", bid);
    }
}
