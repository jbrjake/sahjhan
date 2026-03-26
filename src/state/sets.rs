// src/state/sets.rs

/// Status of an individual set member.
pub struct MemberStatus {
    pub name: String,
    pub done: bool,
}

/// Aggregated status of a completion set.
pub struct SetStatus {
    pub name: String,
    pub total: usize,
    pub completed: usize,
    pub members: Vec<MemberStatus>,
}
