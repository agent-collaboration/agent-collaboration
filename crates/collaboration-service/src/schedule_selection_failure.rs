//! Schedule selection never converts unknown local custody into a central mutation.
use collaboration_protocol::{OperationId, ScheduleId};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ScheduleSelectionFailure {
    #[error("{message}")]
    HandoverPending {
        operation_id: OperationId,
        schedule_id: ScheduleId,
        message: String,
    },
    #[error("{message}")]
    LocalStoreUnavailable {
        #[serde(deserialize_with = "Option::deserialize")]
        operation_id: Option<OperationId>,
        #[serde(deserialize_with = "Option::deserialize")]
        schedule_id: Option<ScheduleId>,
        message: String,
    },
}
impl ScheduleSelectionFailure {
    #[must_use]
    pub fn operation_id(&self) -> Option<&OperationId> {
        match self {
            Self::HandoverPending { operation_id, .. } => Some(operation_id),
            Self::LocalStoreUnavailable { operation_id, .. } => operation_id.as_ref(),
        }
    }
    #[must_use]
    pub fn schedule_id(&self) -> Option<&ScheduleId> {
        match self {
            Self::HandoverPending { schedule_id, .. } => Some(schedule_id),
            Self::LocalStoreUnavailable { schedule_id, .. } => schedule_id.as_ref(),
        }
    }
}
