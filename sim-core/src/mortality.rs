//! Population mortality and growth mechanics.
//!
//! Pops die when food satisfaction is low, creating labor scarcity that
//! drives wages up. This closes the feedback loop between prices and wages.

use rand::Rng;

/// Probability of death given food satisfaction level.
///
/// Scales super-linearly as satisfaction drops below 1.0:
/// - 99% satisfaction → ~0.01% death chance
/// - 80% satisfaction → ~4% death chance
/// - 50% satisfaction → ~28% death chance
/// - 5% satisfaction → ~99% death chance (capped)
///
/// Formula: `(deficit / 0.95)^2` where `deficit = 1 - satisfaction`
pub fn death_probability(food_satisfaction: f64) -> f64 {
    if food_satisfaction >= 1.0 {
        0.0
    } else if food_satisfaction <= 0.0 {
        0.99 // Cap at 99% to allow slim survival chance
    } else {
        let deficit = 1.0 - food_satisfaction;
        // Quadratic scaling, normalized so deficit=0.95 gives p=1.0
        let p = (deficit / 0.95).powi(2);
        p.min(0.99)
    }
}

/// Probability of growth (new pop spawning) given food satisfaction level.
///
/// Much harder to grow than shrink (10x lower probability for same deviation).
/// Only possible when satisfaction > 1.0 (excess food).
///
/// - 150% satisfaction → ~2.8% growth chance
/// - 200% satisfaction → ~10% growth chance (capped)
pub fn growth_probability(food_satisfaction: f64) -> f64 {
    if food_satisfaction <= 1.0 {
        0.0
    } else {
        let excess = food_satisfaction - 1.0;
        // Same formula as death but 10x smaller, capped at 10%
        let p = (excess / 0.95).powi(2) * 0.1;
        p.min(0.10)
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
        // At 100%+ satisfaction, no death
        assert_eq!(death_probability(1.0), 0.0);
        assert_eq!(death_probability(1.5), 0.0);

        // At 99% satisfaction, very low death chance
        let p99 = death_probability(0.99);
        assert!(p99 < 0.001, "p99 = {}", p99);

        // At 80% satisfaction, low but noticeable
        let p80 = death_probability(0.80);
        assert!(p80 > 0.01 && p80 < 0.10, "p80 = {}", p80);

        // At 50% satisfaction, significant
        let p50 = death_probability(0.50);
        assert!(p50 > 0.20 && p50 < 0.35, "p50 = {}", p50);

        // At 5% satisfaction, near certain death
        let p05 = death_probability(0.05);
        assert!(p05 > 0.95, "p05 = {}", p05);

        // At 0% satisfaction, capped at 99%
        assert_eq!(death_probability(0.0), 0.99);
    }

    #[test]
    fn test_growth_probability_curve() {
        // At 100% or below, no growth
        assert_eq!(growth_probability(1.0), 0.0);
        assert_eq!(growth_probability(0.5), 0.0);

        // At 150% satisfaction, modest growth chance
        let p150 = growth_probability(1.5);
        assert!(p150 > 0.02 && p150 < 0.04, "p150 = {}", p150);

        // At 200% satisfaction, higher but capped
        let p200 = growth_probability(2.0);
        assert!(p200 > 0.08 && p200 <= 0.10, "p200 = {}", p200);

        // Growth is much smaller than death for same deviation
        let death_at_50 = death_probability(0.50); // deficit = 0.5
        let growth_at_150 = growth_probability(1.50); // excess = 0.5
        assert!(
            growth_at_150 < death_at_50 * 0.2,
            "growth {} should be much less than death {}",
            growth_at_150,
            death_at_50
        );
    }

    #[test]
    fn test_check_mortality_distribution() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        // At 50% satisfaction, should see mostly deaths
        let mut deaths = 0;
        let trials = 1000;

        for _ in 0..trials {
            match check_mortality(&mut rng, 0.5) {
                MortalityOutcome::Dies => deaths += 1,
                MortalityOutcome::Survives | MortalityOutcome::Grows => {}
            }
        }

        // Expect ~28% deaths
        let death_rate = deaths as f64 / trials as f64;
        assert!(
            death_rate > 0.20 && death_rate < 0.36,
            "death_rate = {}",
            death_rate
        );
    }
}
