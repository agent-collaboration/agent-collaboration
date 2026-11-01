//! Imported configuration preserves local choices without inventing executable timing.
use crate::{
    CodexGeneration, ExecutionDestination, FiringMessageOrigin, ImportedContinuity, InstructionId,
    MessageDelivery, ObservationTimestamp, PositiveSeconds, ScheduleDefinition, SessionRef,
    TimingRequest, WakeState,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("allOf" = [{"required":["importedContinuity"]}]))]
pub struct AutomationImportHeader {
    pub anchor_at: ObservationTimestamp,
    pub created_at: ObservationTimestamp,
    pub updated_at: ObservationTimestamp,
    #[serde(deserialize_with = "Option::deserialize")]
    pub imported_continuity: Option<ImportedContinuity>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("allOf" = [{"required":["generationGuard"]}]))]
pub struct ImportedWakeConfiguration {
    pub target: SessionRef,
    pub origin: FiringMessageOrigin,
    pub delivery: MessageDelivery,
    #[serde(deserialize_with = "Option::deserialize")]
    pub generation_guard: Option<CodexGeneration>,
    pub expires_at: ObservationTimestamp,
    pub state: WakeState,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedWakeDefinition {
    pub configuration: ImportedWakeConfiguration,
    #[schemars(schema_with = "crate::automation_timing_contract::central_timing_schema")]
    pub timing: TimingRequest,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("allOf" = [{"required":["executionTimeoutSeconds","model","effort"]}]))]
pub struct SuspendedScheduleConfiguration {
    pub instruction_id: InstructionId,
    pub enabled: bool,
    pub destination: ExecutionDestination,
    #[serde(deserialize_with = "Option::deserialize")]
    pub execution_timeout_seconds: Option<PositiveSeconds>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub model: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub effort: Option<String>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SuspendedAutomationConfiguration {
    Schedule(SuspendedScheduleConfiguration),
    Wake(ImportedWakeConfiguration),
}

impl SuspendedAutomationConfiguration {
    #[must_use]
    pub fn kind(&self) -> crate::AutomationKind {
        match self {
            Self::Schedule(_) => crate::AutomationKind::Schedule,
            Self::Wake(_) => crate::AutomationKind::Wake,
        }
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuspendedAutomationDefinition {
    pub legacy_timing: TimingRequest,
    pub configuration: SuspendedAutomationConfiguration,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationImportDefinition {
    Schedule(
        #[schemars(schema_with = "crate::automation_custody::frozen_schedule_schema")]
        ScheduleDefinition,
    ),
    Wake(ImportedWakeDefinition),
    Suspended(SuspendedAutomationDefinition),
}

impl AutomationImportDefinition {
    #[must_use]
    pub fn kind(&self) -> crate::AutomationKind {
        match self {
            Self::Schedule(_) => crate::AutomationKind::Schedule,
            Self::Wake(_) => crate::AutomationKind::Wake,
            Self::Suspended(definition) => definition.configuration.kind(),
        }
    }
}
