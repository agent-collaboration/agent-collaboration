//! S6 reads expose the recorded central fault without decoding an executable definition.
use crate::collaboration_rejection::{
    CollaborationRejection, CollaborationRejectionReason, PublishedRejection,
};
use collaboration_protocol::FaultedAutomationSnapshot;
use schemars::JsonSchema;
use serde::Serialize;
pub(crate) const FAULTED_AUTOMATION_MESSAGE: &str =
    "Automation is faulted; inspect the attached catalog fault.";
pub(crate) const SUSPENDED_AUTOMATION_MESSAGE: &str =
    "Automation is suspended; repair its timing before scheduling new work.";

#[derive(Clone, Debug, JsonSchema, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
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
    pub fn suspended(suspended: collaboration_protocol::SuspendedAutomationSnapshot) -> Self {
        Self::Suspended {
            message: SUSPENDED_AUTOMATION_MESSAGE.into(),
            suspended: Box::new(suspended),
        }
    }
    #[must_use]
    pub fn faulted(fault: FaultedAutomationSnapshot) -> Self {
        Self::Faulted {
            message: FAULTED_AUTOMATION_MESSAGE.into(),
            fault,
        }
    }
}
impl CollaborationRejection for CatalogReadFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        None
    }
    fn published_rejection(&self) -> PublishedRejection {
        PublishedRejection::typed(PublishedRejection::OPERATION_FAILED, self.to_string(), self)
    }
}
