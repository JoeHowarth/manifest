// === NEEDS & UTILITY ===

const SUBSISTENCE_SURPLUS_END_RATIO: f64 = 1.25;
const SUBSISTENCE_SURPLUS_MU_SCALE: f64 = 0.15;

pub enum UtilityCurve {
    /// Essentials: high marginal utility until satisfied, then drops
    Subsistence { requirement: f64, steepness: f64 },

    /// Comforts: smooth diminishing returns
    LogDiminishing { scale: f64 },

    /// Luxuries: only kicks in above baseline, convex initially
    LuxuryThreshold { threshold: f64, scale: f64 },

    /// Status goods: relative to neighbors/expectations
    Positional { reference: f64, sensitivity: f64 },
}

impl UtilityCurve {
    pub fn marginal_utility(&self, current_satisfaction: f64) -> f64 {
        match self {
            Self::Subsistence {
                requirement,
                steepness,
            } => {
                let req = requirement.max(0.001);
                let ratio = current_satisfaction / req;
                if ratio < 1.0 {
                    steepness * (1.0 - ratio).powi(2)
                } else if ratio < SUBSISTENCE_SURPLUS_END_RATIO {
                    // Allow a small diminishing "surplus calories" tail above
                    // survival so growth can occur when food is plentiful.
                    let t = (SUBSISTENCE_SURPLUS_END_RATIO - ratio)
                        / (SUBSISTENCE_SURPLUS_END_RATIO - 1.0);
                    (steepness * SUBSISTENCE_SURPLUS_MU_SCALE * t).max(0.0)
                } else {
                    // Surplus appetite saturates quickly above subsistence.
                    0.0
                }
            }
            Self::LogDiminishing { scale } => scale / (1.0 + current_satisfaction),
            Self::LuxuryThreshold { threshold, scale } => {
                if current_satisfaction < *threshold {
                    0.0
                } else {
                    scale / (1.0 + current_satisfaction - threshold)
                }
            }
            Self::Positional {
                reference,
                sensitivity,
            } => sensitivity * (reference - current_satisfaction).tanh(),
        }
    }
}

pub struct Need {
    pub id: String,
    pub utility_curve: UtilityCurve,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsistence_mu_positive_at_survival() {
        let curve = UtilityCurve::Subsistence {
            requirement: 1.0,
            steepness: 5.0,
        };
        let mu = curve.marginal_utility(1.0);
        assert!(mu > 0.0, "MU at survival threshold should be positive");
    }

    #[test]
    fn subsistence_mu_declines_across_surplus_band() {
        let curve = UtilityCurve::Subsistence {
            requirement: 1.0,
            steepness: 5.0,
        };
        let mu_at_one = curve.marginal_utility(1.0);
        let mu_mid = curve.marginal_utility((1.0 + SUBSISTENCE_SURPLUS_END_RATIO) * 0.5);
        let mu_end = curve.marginal_utility(SUBSISTENCE_SURPLUS_END_RATIO);
        assert!(
            mu_at_one > mu_mid,
            "surplus MU should diminish with extra intake"
        );
        assert!(mu_mid > mu_end, "surplus MU should approach zero near cap");
    }

    #[test]
    fn subsistence_mu_zero_after_surplus_cap() {
        let curve = UtilityCurve::Subsistence {
            requirement: 1.0,
            steepness: 5.0,
        };
        let mu = curve.marginal_utility(SUBSISTENCE_SURPLUS_END_RATIO + 0.05);
        assert_eq!(mu, 0.0, "MU should be zero past surplus cap");
    }
}
