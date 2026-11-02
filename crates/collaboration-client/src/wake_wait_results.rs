//! The SDK retains the service's central Firing evidence without making a legacy fire receipt.
use collaboration_protocol::{FaultedAutomationSnapshot, FiringId, FiringSnapshot, WakeupId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        firing_id: Option<FiringId>,
    },
}
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CentralWakeWaitResult {
    pub wakeup_id: WakeupId,
    pub outcome: CentralWakeWaitOutcome,
}
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
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
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WakeFirstFireResponse {
    Suspended {
        message: String,
        suspended: Box<collaboration_protocol::SuspendedAutomationSnapshot>,
    },
    Local {
        value: collaboration_protocol::FireReceipt,
    },
    Central {
        value: CentralWakeWaitResult,
    },
    Faulted {
        message: String,
        fault: FaultedAutomationSnapshot,
    },
}
