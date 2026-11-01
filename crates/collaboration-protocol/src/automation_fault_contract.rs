//! A fault remains inspectable when its stored definition cannot be decoded.
use crate::{AutomationId, AutomationKind, AutomationRefusal, ChangeId, ObservationTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationFault {
    pub error: AutomationRefusal,
    pub faulted_at: ObservationTimestamp,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FaultedAutomationSnapshot {
    pub automation_id: AutomationId,
    pub kind: AutomationKind,
    pub revision: ChangeId,
    pub fault: AutomationFault,
}
