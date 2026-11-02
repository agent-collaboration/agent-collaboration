//! Ownership selection failures never imply that a local or central mutation was dispatched.
use collaboration_protocol::{OperationId, WakeupId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WakeSelectionFailure {
    #[error("{message}")]
    HandoverPending {
        operation_id: OperationId,
        wakeup_id: WakeupId,
        message: String,
    },
    #[error("{message}")]
    LocalStoreUnavailable {
        #[serde(deserialize_with = "Option::deserialize")]
        operation_id: Option<OperationId>,
        #[serde(deserialize_with = "Option::deserialize")]
        wakeup_id: Option<WakeupId>,
        message: String,
    },
}
impl WakeSelectionFailure {
    #[must_use]
    pub fn operation_id(&self) -> Option<&OperationId> {
        match self {
            Self::HandoverPending { operation_id, .. } => Some(operation_id),
            Self::LocalStoreUnavailable { operation_id, .. } => operation_id.as_ref(),
        }
    }
    #[must_use]
    pub fn wakeup_id(&self) -> Option<&WakeupId> {
        match self {
            Self::HandoverPending { wakeup_id, .. } => Some(wakeup_id),
            Self::LocalStoreUnavailable { wakeup_id, .. } => wakeup_id.as_ref(),
        }
    }
}
