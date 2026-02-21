//! Facility labor bidding logic.
//!
//! Facilities adjust their wage bids based on whether they filled slots last tick:
//! - Bid above MVP → lower toward MVP (never overpay)
//! - Unfilled profitable slots + workers to attract → raise bid
//! - Filled slots → lower bid (test if you can pay less)
//! - Otherwise → hold steady
//!
//! Monopsony awareness: facilities only raise bids when there are workers to
//! attract (unemployed or at competing employers). A single employer with all
//! workers already hired won't bid up against itself.
//!
//! The lowering path is the same for monopsony and competition. In competition,
//! lowering too far causes worker loss at the auction → unfilled slots → raise
//! path fires, creating oscillation around the competitive equilibrium. In
//! monopsony, workers have no alternative, so wages fall to reservation.

use std::collections::HashMap;

use rand::Rng;

use super::skills::SkillId;
use crate::types::Price;

/// Probability of lowering wages each tick when all slots are filled.
/// Models employer inertia — "things are working, why change?" is the common case.
/// Effective grind rate = LOWER_PROB × 5% ≈ 1.5%/tick on average.
pub const FILLED_LOWER_PROB: f64 = 0.3;

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
    /// Current MVP for this skill at this facility (always set).
    /// Used to detect when bid > MVP (employer overpaying).
    pub mvp: Price,
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
    /// - `mvp`: Current MVP for this skill (marginal_output × output_price)
    pub fn record_outcome(
        &mut self,
        skill: SkillId,
        filled: u32,
        profitable_unfilled: u32,
        marginal_profitable_mvp: Option<Price>,
        mvp: Price,
    ) {
        self.last_outcome.insert(
            skill,
            SkillOutcome {
                filled,
                profitable_unfilled,
                marginal_profitable_mvp,
                mvp,
            },
        );
    }

    /// Adjust bid for next tick based on last tick's outcome.
    ///
    /// - `rng`: Random number generator for stochastic lowering
    /// - `wage_ema`: Current market wage EMA (used as floor reference)
    /// - `can_attract_workers`: True if there are workers employable by raising
    ///   the bid — either unemployed workers or workers at competing employers.
    ///   False in a monopsony where all workers are already hired by this owner.
    pub fn adjust_bid(
        &mut self,
        rng: &mut impl Rng,
        skill: SkillId,
        wage_ema: Price,
        can_attract_workers: bool,
    ) {
        let current_bid = self.get_bid(skill, wage_ema);
        let outcome = self.last_outcome.get(&skill).cloned().unwrap_or_default();
        let mvp = outcome.mvp;

        let new_bid = if current_bid > mvp && mvp > 0.0 {
            // Overpaying: bid exceeds what a worker produces. Lower toward MVP.
            // This takes priority — no rational employer pays more than output value.
            (current_bid * 0.95).max(mvp)
        } else if outcome.profitable_unfilled > 0 && can_attract_workers {
            // Unfilled profitable slots AND there are workers to attract
            // (either unemployed or at competing employers): raise bid immediately.
            // Unfilled slots = lost production right now, so act urgently.
            let cap = outcome.marginal_profitable_mvp.unwrap_or(current_bid);
            (current_bid * 1.2).min(cap)
        } else if outcome.filled > 0 && rng.random_bool(FILLED_LOWER_PROB) {
            // Filled: stochastically try to pay less.
            // Models employer inertia — most ticks they leave wages alone.
            // When they do cut, 5% reduction floored at half of market rate.
            // In competition, lowering too far → lose workers → raise path fires.
            // In monopsony, wages drift to reservation, but slowly.
            let floor = wage_ema * 0.5;
            (current_bid * 0.95).max(floor)
        } else {
            // No fills, no profitable unfilled, or chose not to lower: hold
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

    /// RNG that always triggers lowering (returns 0.0 for random_bool checks)
    struct AlwaysLowerRng;
    impl rand::RngCore for AlwaysLowerRng {
        fn next_u32(&mut self) -> u32 { 0 }
        fn next_u64(&mut self) -> u64 { 0 }
        fn fill_bytes(&mut self, dest: &mut [u8]) { dest.fill(0); }
    }

    /// RNG that never triggers lowering (returns max for random_bool checks)
    struct NeverLowerRng;
    impl rand::RngCore for NeverLowerRng {
        fn next_u32(&mut self) -> u32 { u32::MAX }
        fn next_u64(&mut self) -> u64 { u64::MAX }
        fn fill_bytes(&mut self, dest: &mut [u8]) { dest.fill(0xFF); }
    }

    #[test]
    fn profitable_unfilled_slots_raise_bid() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 10.0);
        state.record_outcome(laborer, 1, 2, Some(50.0), 50.0);

        // Raise path doesn't use RNG
        state.adjust_bid(&mut AlwaysLowerRng, laborer, 10.0, true);

        assert_eq!(state.bids.get(&laborer), Some(&12.0));
    }

    #[test]
    fn overpaying_lowers_bid_toward_mvp() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        // Bid = 50 but MVP is only 40 → overpaying (deterministic, no RNG)
        state.bids.insert(laborer, 50.0);
        state.record_outcome(laborer, 2, 0, None, 40.0);

        state.adjust_bid(&mut NeverLowerRng, laborer, 10.0, true);

        assert_eq!(state.bids.get(&laborer), Some(&47.5));
    }

    #[test]
    fn filled_bid_lowers_past_mvp_to_floor() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        // Bid starts at 100, MVP is 40, wage_ema = 10 → floor = 5.
        // Use AlwaysLowerRng to guarantee the stochastic path fires.
        state.bids.insert(laborer, 100.0);

        for _ in 0..100 {
            state.record_outcome(laborer, 2, 0, None, 40.0);
            state.adjust_bid(&mut AlwaysLowerRng, laborer, 10.0, true);
        }

        let bid = *state.bids.get(&laborer).unwrap();
        assert!(
            (bid - 5.0).abs() < 0.01,
            "bid should converge to floor=5.0, got {}",
            bid
        );
    }

    #[test]
    fn filled_slots_lower_bid_when_rng_fires() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 20.0);
        state.record_outcome(laborer, 3, 0, None, 30.0);

        // AlwaysLowerRng → stochastic lower fires
        state.adjust_bid(&mut AlwaysLowerRng, laborer, 10.0, true);
        assert_eq!(state.bids.get(&laborer), Some(&19.0));
    }

    #[test]
    fn filled_slots_hold_when_rng_does_not_fire() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 20.0);
        state.record_outcome(laborer, 3, 0, None, 30.0);

        // NeverLowerRng → stochastic lower doesn't fire, bid holds
        state.adjust_bid(&mut NeverLowerRng, laborer, 10.0, true);
        assert_eq!(state.bids.get(&laborer), Some(&20.0));
    }

    #[test]
    fn bid_capped_at_marginal_mvp() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 90.0);
        state.record_outcome(laborer, 0, 3, Some(100.0), 100.0);

        state.adjust_bid(&mut AlwaysLowerRng, laborer, 10.0, true);

        assert_eq!(state.bids.get(&laborer), Some(&100.0));
    }

    #[test]
    fn bid_has_floor() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 6.0);
        state.record_outcome(laborer, 3, 0, None, 100.0);

        for _ in 0..20 {
            state.adjust_bid(&mut AlwaysLowerRng, laborer, 10.0, true);
        }

        let bid = state.bids.get(&laborer).unwrap();
        assert!(*bid >= 5.0, "bid {} should be >= floor 5.0", bid);
    }

    #[test]
    fn monopsony_lowers_when_filled() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        state.bids.insert(laborer, 10.0);
        state.record_outcome(laborer, 3, 2, Some(50.0), 50.0);

        state.adjust_bid(&mut AlwaysLowerRng, laborer, 10.0, false);

        assert_eq!(state.bids.get(&laborer), Some(&9.5));
    }

    #[test]
    fn monopsony_lowers_when_overpaying() {
        let mut state = FacilityBidState::new();
        let laborer = skill(1);

        // Overpaying is deterministic, then filled+stochastic lowers to floor
        state.bids.insert(laborer, 100.0);

        for _ in 0..100 {
            state.record_outcome(laborer, 3, 0, None, 40.0);
            state.adjust_bid(&mut AlwaysLowerRng, laborer, 10.0, false);
        }

        let bid = *state.bids.get(&laborer).unwrap();
        assert!(
            (bid - 5.0).abs() < 0.01,
            "monopsony bid should converge to floor=5.0, got {}",
            bid
        );
    }
}
