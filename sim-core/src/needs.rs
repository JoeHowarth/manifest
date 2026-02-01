// === NEEDS & UTILITY ===

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
                let ratio = current_satisfaction / requirement;
                if ratio < 1.0 {
                    steepness * (1.0 - ratio).powi(2)
                } else {
                    0.01 / ratio
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
