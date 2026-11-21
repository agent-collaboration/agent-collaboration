//! The outcome and capability for completing an event and publishing its alarm.
use central_storage::{AlarmPublication, StoreFailure};
use std::future::Future;

/// What one due batch did, counted per unit kind.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlarmResult {
    pub wakes_fired: usize,
    pub automations_fired: usize,
    pub windows_fired: usize,
    pub workspaces_drained: usize,
}

impl std::ops::AddAssign for AlarmResult {
    fn add_assign(&mut self, other: Self) {
        self.wakes_fired += other.wakes_fired;
        self.automations_fired += other.automations_fired;
        self.windows_fired += other.windows_fired;
        self.workspaces_drained += other.workspaces_drained;
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventCompletion {
    pub work: AlarmResult,
    pub publication: AlarmPublication,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("the event's alarm was not published: {failure}")]
pub struct AlarmNotPublished {
    pub failure: StoreFailure,
}

pub trait EventCompletionHook: Send + Sync {
    fn complete_event(
        &self,
    ) -> impl Future<Output = Result<EventCompletion, AlarmNotPublished>> + Send;
}
