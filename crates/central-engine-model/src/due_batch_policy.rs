#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DueBatchPolicy {
    pub spacing: chrono::Duration,
    pub look_ahead: chrono::Duration,
    pub per_kind_cap: usize,
}

impl DueBatchPolicy {
    pub const DEFAULT: Self = Self {
        spacing: chrono::Duration::seconds(30),
        look_ahead: chrono::Duration::seconds(50),
        per_kind_cap: 250,
    };
}

impl Default for DueBatchPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}
