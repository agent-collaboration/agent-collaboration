//! Public result envelopes preserve central snapshots and actual already-sent Firing IDs.
use collaboration_protocol::FiringId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogMutationResult {
    pub automation: CatalogAutomationResult,
    pub already_sent: Vec<FiringId>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WakeCreateResponse {
    Central {
        value: collaboration_protocol::CentralWakeSnapshot,
    },
    /// Interim recovery only: remove after the 32-day retention following release.
    Legacy {
        value: collaboration_protocol::WakeSnapshot,
    },
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
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

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WakeMutationResponse {
    Local(collaboration_protocol::WakeMutationResult),
    Central(CatalogMutationResult),
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
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

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CatalogAutomationResult {
    Suspended(collaboration_protocol::SuspendedAutomationSnapshot),
    Schedule(crate::CentralScheduleSnapshot),
    Wake(collaboration_protocol::CentralWakeSnapshot),
    Faulted(collaboration_protocol::FaultedAutomationSnapshot),
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ScheduleMutationResponse {
    Local(collaboration_protocol::ScheduleSnapshot),
    Central(CatalogMutationResult),
}
