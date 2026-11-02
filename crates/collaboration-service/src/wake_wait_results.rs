//! Local cursors and central Wake-ID observations have different evidence and result shapes.
use collaboration_protocol::{FaultedAutomationSnapshot, FiringId, FiringSnapshot, WakeupId};
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CentralWakeWaitOutcome {
    Fired {
        firing: Box<FiringSnapshot>,
    },
    Paused {},
    Cancelled {},
    Expired {},
    FinishedWithoutFiring {},
    Pending {
        #[serde(skip_serializing_if = "Option::is_none")]
        firing_id: Option<FiringId>,
    },
}
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CentralWakeWaitResult {
    pub wakeup_id: WakeupId,
    pub outcome: CentralWakeWaitOutcome,
}
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WakeWaitResponse {
    Suspended {
        message: String,
        suspended: Box<collaboration_protocol::SuspendedAutomationSnapshot>,
    },
    Local {
        value: collaboration_protocol::WakeWaitResult,
    },
    Central {
        value: CentralWakeWaitResult,
    },
    Faulted {
        message: String,
        fault: FaultedAutomationSnapshot,
    },
}
impl WakeWaitResponse {
    pub(crate) fn suspended(
        suspended: collaboration_protocol::SuspendedAutomationSnapshot,
    ) -> Self {
        Self::Suspended {
            message: crate::catalog_read_failure::SUSPENDED_AUTOMATION_MESSAGE.into(),
            suspended: Box::new(suspended),
        }
    }
    pub(crate) fn faulted(fault: FaultedAutomationSnapshot) -> Self {
        Self::Faulted {
            message: crate::catalog_read_failure::FAULTED_AUTOMATION_MESSAGE.into(),
            fault,
        }
    }
    pub(crate) fn pending(wakeup_id: WakeupId) -> Self {
        Self::Central {
            value: CentralWakeWaitResult {
                wakeup_id,
                outcome: CentralWakeWaitOutcome::Pending { firing_id: None },
            },
        }
    }
}
