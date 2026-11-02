//! Public catalog mutation evidence references the central-owned domain types.
use collaboration_protocol::FiringId;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogMutationResult {
    pub automation: CatalogAutomationResult,
    pub already_sent: Vec<FiringId>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WakeCreateResponse {
    Central {
        value: collaboration_protocol::CentralWakeSnapshot,
    },
    /// Interim recovery only: remove after the 32-day retention following release.
    Legacy {
        value: collaboration_protocol::WakeSnapshot,
    },
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScheduleReadResponse {
    Suspended {
        message: String,
        suspended: collaboration_protocol::SuspendedAutomationSnapshot,
    },
    Local {
        value: collaboration_protocol::ScheduleSnapshot,
    },
    Schedule {
        value: crate::CentralScheduleSnapshot,
    },
    Faulted {
        message: String,
        fault: collaboration_protocol::FaultedAutomationSnapshot,
    },
}
impl ScheduleReadResponse {
    pub(crate) fn from_catalog(
        automation: collaboration_protocol::CatalogAutomation,
    ) -> Option<Self> {
        match automation {
            collaboration_protocol::CatalogAutomation::Suspended(suspended)
                if suspended.kind == collaboration_protocol::AutomationKind::Schedule =>
            {
                Some(Self::Suspended {
                    message: crate::catalog_read_failure::SUSPENDED_AUTOMATION_MESSAGE.into(),
                    suspended,
                })
            }
            collaboration_protocol::CatalogAutomation::Schedule(value) => Some(Self::Schedule {
                value: value.into(),
            }),
            collaboration_protocol::CatalogAutomation::Faulted(fault)
                if fault.kind == collaboration_protocol::AutomationKind::Schedule =>
            {
                Some(Self::Faulted {
                    message: crate::catalog_read_failure::FAULTED_AUTOMATION_MESSAGE.into(),
                    fault,
                })
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum WakeMutationResponse {
    Local(collaboration_protocol::WakeMutationResult),
    Central(CatalogMutationResult),
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WakeReadResponse {
    Suspended {
        message: String,
        suspended: collaboration_protocol::SuspendedAutomationSnapshot,
    },
    Local {
        value: collaboration_protocol::WakeSnapshot,
    },
    Central {
        value: collaboration_protocol::CentralWakeSnapshot,
    },
    Faulted {
        message: String,
        fault: collaboration_protocol::FaultedAutomationSnapshot,
    },
}
impl WakeReadResponse {
    pub(crate) fn suspended(
        suspended: collaboration_protocol::SuspendedAutomationSnapshot,
    ) -> Self {
        Self::Suspended {
            message: crate::catalog_read_failure::SUSPENDED_AUTOMATION_MESSAGE.into(),
            suspended,
        }
    }
    pub(crate) fn faulted(fault: collaboration_protocol::FaultedAutomationSnapshot) -> Self {
        Self::Faulted {
            message: crate::catalog_read_failure::FAULTED_AUTOMATION_MESSAGE.into(),
            fault,
        }
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum CatalogAutomationResult {
    Suspended(collaboration_protocol::SuspendedAutomationSnapshot),
    Schedule(crate::CentralScheduleSnapshot),
    Wake(collaboration_protocol::CentralWakeSnapshot),
    Faulted(collaboration_protocol::FaultedAutomationSnapshot),
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ScheduleMutationResponse {
    Local(collaboration_protocol::ScheduleSnapshot),
    Central(CatalogMutationResult),
}

impl From<collaboration_protocol::CatalogAutomation> for CatalogAutomationResult {
    fn from(value: collaboration_protocol::CatalogAutomation) -> Self {
        match value {
            collaboration_protocol::CatalogAutomation::Suspended(value) => Self::Suspended(value),
            collaboration_protocol::CatalogAutomation::Schedule(value) => {
                Self::Schedule(value.into())
            }
            collaboration_protocol::CatalogAutomation::Wake(value) => Self::Wake(value),
            collaboration_protocol::CatalogAutomation::Faulted(value) => Self::Faulted(value),
        }
    }
}
