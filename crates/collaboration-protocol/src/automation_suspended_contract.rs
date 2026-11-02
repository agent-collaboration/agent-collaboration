//! A suspended move exposes preserved configuration without executable timing.
use crate::{
    AutomationId, AutomationImportHeader, AutomationKind, ChangeId, ContentReference,
    ImportedWakeConfiguration, SuspendedScheduleConfiguration, TimingRequest, WakeCreator,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuspendedAutomationSnapshot {
    pub automation_id: AutomationId,
    pub kind: AutomationKind,
    pub revision: ChangeId,
    pub legacy_timing: TimingRequest,
    pub configuration: SuspendedAutomationSnapshotConfiguration,
    pub header: AutomationImportHeader,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SuspendedAutomationSnapshotConfiguration {
    Schedule(SuspendedScheduleConfiguration),
    Wake {
        configuration: ImportedWakeConfiguration,
        created_by: WakeCreator,
        content: ContentReference,
    },
}
