use std::collections::{HashMap, HashSet};

use crate::pops::types::Price;

// === SKILL TYPES ===

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct SkillId(pub u32);

pub struct SkillDef {
    pub id: SkillId,
    pub name: String,
    pub parent: Option<SkillId>, // None for root (e.g., Laborer)
}

impl SkillDef {
    /// Returns all ancestor skills (including self)
    pub fn skill_chain<'a>(&self, all_skills: &'a HashMap<SkillId, SkillDef>) -> Vec<&'a SkillDef> {
        let mut chain = vec![];
        let mut current = all_skills.get(&self.id);
        while let Some(skill) = current {
            chain.push(skill);
            current = skill.parent.and_then(|p| all_skills.get(&p));
        }
        chain
    }
}

// === WORKER ===

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct WorkerId(pub u32);

pub struct Worker {
    pub id: WorkerId,
    pub skills: HashSet<SkillId>, // all skills including inherited
    pub min_wage: Price,          // reservation wage
}
