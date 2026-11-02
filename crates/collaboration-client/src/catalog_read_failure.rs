//! The service owns this fault envelope; the fault itself remains the shared central type.
use collaboration_protocol::FaultedAutomationSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CatalogReadFailure {
    #[error("{message}")]
    Suspended {
        message: String,
        suspended: Box<collaboration_protocol::SuspendedAutomationSnapshot>,
    },
    #[error("{message}")]
    Faulted {
        message: String,
        fault: FaultedAutomationSnapshot,
    },
}
impl CatalogReadFailure {
    #[must_use]
    pub fn fault(&self) -> Option<&FaultedAutomationSnapshot> {
        match self {
            Self::Faulted { fault, .. } => Some(fault),
            Self::Suspended { .. } => None,
        }
    }
    pub fn kind(&self) -> collaboration_protocol::AutomationKind {
        match self {
            Self::Faulted { fault, .. } => fault.kind,
            Self::Suspended { suspended, .. } => suspended.kind,
        }
    }
    pub fn automation_id(&self) -> &collaboration_protocol::AutomationId {
        match self {
            Self::Faulted { fault, .. } => &fault.automation_id,
            Self::Suspended { suspended, .. } => &suspended.automation_id,
        }
    }
}
