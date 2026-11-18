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

/// Completes an event through the composed engine and returns its publication.
pub trait EventCompletionHook: Send + Sync {
    fn complete_event(
        &self,
    ) -> impl Future<Output = Result<EventCompletion, AlarmNotPublished>> + Send;
}
