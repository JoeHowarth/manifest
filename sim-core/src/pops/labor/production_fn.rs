use std::collections::HashMap;

use super::skills::SkillId;

// === PRODUCTION FUNCTION ===

/// Defines how a facility converts workers into output
pub trait ProductionFn {
    /// Compute output given worker counts by skill
    fn compute(&self, workers: &HashMap<SkillId, u32>) -> f64;

    /// Skills this production function uses
    fn relevant_skills(&self) -> Vec<SkillId>;
}

/// Simple production function with complementarity
/// Output = sum of individual contributions + bonus for combinations
pub struct ComplementaryProductionFn {
    /// Base output per worker of each skill type
    pub base_output: HashMap<SkillId, f64>,

    /// Bonus output for having workers of multiple skill types
    /// Key: (skill_a, skill_b), Value: bonus per pair
    pub complementarity_bonus: HashMap<(SkillId, SkillId), f64>,

    /// Maximum workers before diminishing returns kick in
    pub max_optimal_capacity: HashMap<SkillId, u32>,

    /// How quickly output diminishes after max capacity (0 = cliff, 1 = gradual)
    pub diminishing_rate: f64,
}

impl ProductionFn for ComplementaryProductionFn {
    fn compute(&self, workers: &HashMap<SkillId, u32>) -> f64 {
        let mut output = 0.0;

        // Base output with diminishing returns
        for (skill, &count) in workers {
            let base = self.base_output.get(skill).copied().unwrap_or(0.0);
            let max_cap = self.max_optimal_capacity.get(skill).copied().unwrap_or(10);

            for i in 0..count {
                if i < max_cap {
                    output += base;
                } else {
                    // Linear diminishing returns after capacity
                    let over = i - max_cap + 1;
                    let factor = (1.0 - self.diminishing_rate * over as f64).max(0.0);
                    output += base * factor;
                }
            }
        }

        // Complementarity bonuses
        for ((skill_a, skill_b), bonus) in &self.complementarity_bonus {
            let count_a = workers.get(skill_a).copied().unwrap_or(0);
            let count_b = workers.get(skill_b).copied().unwrap_or(0);
            // Bonus applies to each pair
            let pairs = count_a.min(count_b);
            output += *bonus * pairs as f64;
        }

        output
    }

    fn relevant_skills(&self) -> Vec<SkillId> {
        self.base_output.keys().copied().collect()
    }
}
