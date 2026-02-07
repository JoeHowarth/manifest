//! Population mortality and growth mechanics.
//!
//! Pops die when food satisfaction is low, creating labor scarcity that
//! drives wages up. This closes the feedback loop between prices and wages.

use rand::Rng;

const DEATH_FREE_SATISFACTION: f64 = 0.9;
const SURPLUS_SATISFACTION_CAP: f64 = 1.25;
const MAX_GROWTH_PROBABILITY: f64 = 0.02;

/// Probability of death given food satisfaction level.
///
/// No death above 90% food satisfaction.
/// Below 90%, death risk scales quadratically with deficit.
///
/// Formula below threshold:
/// `0.99 * ((0.9 - satisfaction) / 0.9)^2`
pub fn death_probability(food_satisfaction: f64) -> f64 {
    if food_satisfaction >= DEATH_FREE_SATISFACTION {
        0.0
    } else if food_satisfaction <= 0.0 {
        0.99 // Cap at 99% to allow slim survival chance
    } else {
        let deficit = DEATH_FREE_SATISFACTION - food_satisfaction;
        // Quadratic scaling, normalized so satisfaction=0 gives p=0.99.
        let p = 0.99 * (deficit / DEATH_FREE_SATISFACTION).powi(2);
        p.min(0.99)
    }
}

/// Probability of growth (new pop spawning) given food satisfaction level.
///
/// Growth is possible only with food surplus (satisfaction > 1.0), and
/// increases modestly up to the subsistence-surplus sat cap.
///
/// Above the cap, growth probability is held constant.
pub fn growth_probability(food_satisfaction: f64) -> f64 {
    if food_satisfaction <= 1.0 {
        0.0
    } else {
        let span = (SURPLUS_SATISFACTION_CAP - 1.0).max(0.001);
        let progress = ((food_satisfaction - 1.0) / span).clamp(0.0, 1.0);
        MAX_GROWTH_PROBABILITY * progress
    }
}

/// Result of mortality check for a population
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MortalityOutcome {
    /// Pop survives unchanged
    Survives,
    /// Pop dies (should be removed)
    Dies,
    /// Pop grows (should spawn new pop)
    Grows,
}

/// Check mortality outcome for a pop given their food satisfaction.
/// Uses random roll against death/growth probabilities.
pub fn check_mortality<R: Rng>(rng: &mut R, food_satisfaction: f64) -> MortalityOutcome {
    let roll: f64 = rng.random();

    let p_death = death_probability(food_satisfaction);
    if roll < p_death {
        return MortalityOutcome::Dies;
    }

    let p_growth = growth_probability(food_satisfaction);
    if roll < p_death + p_growth {
        return MortalityOutcome::Grows;
    }

    MortalityOutcome::Survives
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_death_probability_curve() {
        // At >90% satisfaction, no death
        assert_eq!(death_probability(1.0), 0.0);
        assert_eq!(death_probability(0.95), 0.0);
        assert_eq!(death_probability(1.5), 0.0);

        // At 99% satisfaction, still no death (new threshold)
        let p99 = death_probability(0.99);
        assert_eq!(p99, 0.0, "p99 = {}", p99);

        // At 80% satisfaction, low but non-zero
        let p80 = death_probability(0.80);
        assert!(p80 > 0.005 && p80 < 0.03, "p80 = {}", p80);

        // At 50% satisfaction, significant
        let p50 = death_probability(0.50);
        assert!(p50 > 0.15 && p50 < 0.25, "p50 = {}", p50);

        // At 5% satisfaction, very high death risk
        let p05 = death_probability(0.05);
        assert!(p05 > 0.85, "p05 = {}", p05);

        // At 0% satisfaction, capped at 99%
        assert_eq!(death_probability(0.0), 0.99);
    }

    #[test]
    fn test_growth_probability_curve() {
        // At 100% or below, no growth
        assert_eq!(growth_probability(1.0), 0.0);
        assert_eq!(growth_probability(0.5), 0.0);

        // At 110% satisfaction, small growth chance
        let p110 = growth_probability(1.10);
        assert!(p110 > 0.005 && p110 < 0.01, "p110 = {}", p110);

        // At 200% satisfaction, capped at the small max growth rate
        let p200 = growth_probability(2.0);
        assert!(
            p200 > 0.015 && p200 <= MAX_GROWTH_PROBABILITY,
            "p200 = {}",
            p200
        );

        // Growth is much smaller than death for same deviation
        let death_at_50 = death_probability(0.50); // deficit = 0.5
        let growth_at_125 = growth_probability(1.25); // at cap
        assert!(
            growth_at_125 < death_at_50 * 0.2,
            "growth {} should be much less than death {}",
            growth_at_125,
            death_at_50
        );
    }

    #[test]
    fn test_check_mortality_distribution() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        // At 50% satisfaction, should see substantial deaths
        let mut deaths = 0;
        let trials = 1000;

        for _ in 0..trials {
            match check_mortality(&mut rng, 0.5) {
                MortalityOutcome::Dies => deaths += 1,
                MortalityOutcome::Survives | MortalityOutcome::Grows => {}
            }
        }

        // Expect ~20% deaths under current curve
        let death_rate = deaths as f64 / trials as f64;
        assert!(
            death_rate > 0.14 && death_rate < 0.26,
            "death_rate = {}",
            death_rate
        );
    }
}
