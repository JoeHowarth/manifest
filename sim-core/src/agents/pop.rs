use std::collections::{HashMap, HashSet};

use crate::labor::SkillId;
use crate::types::{FacilityKey, GoodId, Price, Quantity};

// === CONSUMPTION ===

pub struct ConsumptionResult {
    pub actual: HashMap<GoodId, Quantity>,
    pub desired: HashMap<GoodId, Quantity>,
}

// === POP ===

/// A population unit (~100 workers + dependents) bound to a settlement.
/// Makes consumption decisions, participates in labor markets as 1 worker.
#[derive(Debug, Clone)]
pub struct Pop {
    pub currency: f64,
    pub stocks: HashMap<GoodId, Quantity>,
    pub desired_consumption_ema: HashMap<GoodId, Quantity>,
    pub need_satisfaction: HashMap<String, f64>,
    /// Smoothed income used as budget for desire discovery and market purchases.
    pub income_ema: f64,

    // Labor market participation
    /// Skills this pop can work as (includes inherited skills)
    pub skills: HashSet<SkillId>,
    /// Minimum acceptable wage (reservation wage)
    pub min_wage: Price,
    /// Current employment: facility this pop works at (if any)
    pub employed_at: Option<FacilityKey>,
    /// Which skill this pop is currently employed under (if any)
    pub employed_skill: Option<SkillId>,
}

impl Pop {
    pub fn new() -> Self {
        Self {
            currency: 1000.0,
            stocks: HashMap::new(),
            desired_consumption_ema: HashMap::new(),
            need_satisfaction: HashMap::new(),
            income_ema: 100.0,
            skills: HashSet::new(),
            min_wage: 1.0, // Default reservation wage
            employed_at: None,
            employed_skill: None,
        }
    }

    pub fn with_currency(mut self, currency: f64) -> Self {
        self.currency = currency;
        self
    }

    pub fn with_stocks(mut self, stocks: HashMap<GoodId, Quantity>) -> Self {
        self.stocks = stocks;
        self
    }

    pub fn with_skills(mut self, skills: impl IntoIterator<Item = SkillId>) -> Self {
        self.skills = skills.into_iter().collect();
        self
    }

    pub fn with_min_wage(mut self, min_wage: Price) -> Self {
        self.min_wage = min_wage;
        self
    }

    /// Update income EMA based on wages received this tick
    pub fn record_income(&mut self, wage: f64) {
        // Blend into EMA: 70% old, 30% new
        self.income_ema = 0.7 * self.income_ema + 0.3 * wage;
    }

    /// Is this pop currently employed?
    pub fn is_employed(&self) -> bool {
        self.employed_at.is_some()
    }
}

impl Default for Pop {
    fn default() -> Self {
        Self::new()
    }
}
