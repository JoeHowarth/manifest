use super::ids::GoodId;

// === GOOD PROFILES ===

pub struct NeedContribution {
    pub need_id: String,
    pub efficiency: f64, // units of need satisfaction per unit good
}

pub struct GoodProfile {
    pub good: GoodId,
    pub contributions: Vec<NeedContribution>,
}
