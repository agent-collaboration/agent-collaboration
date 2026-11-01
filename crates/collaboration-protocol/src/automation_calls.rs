//! The Fleet catalog and custody call families share the existing Session → Fleet edge.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationCall {
    InstructionCreate(crate::InstructionCreateParams),
    InstructionUpdate(crate::InstructionUpdateParams),
    InstructionShow(crate::InstructionShowParams),
    InstructionList(crate::AutomationPageRequest),
    RevisionList(crate::RevisionListRequest),
    ScheduleCreate(crate::ScheduleCreateRequest),
    ScheduleUpdate(crate::ScheduleUpdateRequest),
    ScheduleShow(crate::ScheduleShowRequest),
    ScheduleEnable(crate::ScheduleEnableRequest),
    ScheduleDisable(crate::ScheduleEnableRequest),
    ScheduleCancel(crate::ScheduleEnableRequest),
    WakeSend(crate::WakeSendRequest),
    WakeShow(crate::WakeShowRequest),
    WakePause(crate::WakeMutationRequest),
    WakeResume(crate::WakeMutationRequest),
    WakeCancel(crate::WakeMutationRequest),
    List(crate::AutomationPageRequest),
    PullAutomationBatches(crate::PullAutomationBatches),
    FetchContents(crate::FetchContents),
    AckAutomationBatches(crate::AckAutomationBatches),
    ReportWorkItems(crate::ReportWorkItems),
    LocalRunSettled(crate::LocalRunSettled),
    RemoteSessionCreate(crate::RemoteSessionCreateRequest),
    RemoteSessionCreateStatus { operation_id: crate::OperationId },
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CatalogAutomation {
    Schedule(crate::ScheduleSnapshot),
    Wake(crate::WakeSnapshot),
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationRefusal {
    Validation {
        error: crate::AutomationValidationError,
    },
    OperationConflict {
        operation_id: crate::OperationId,
    },
    WakeExists {
        creator: crate::WakeCreator,
    },
    TargetNeedsUpgrade {
        service_id: crate::UuidIdentity,
    },
    NotFound,
    OriginMismatch,
    DigestMismatch,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationAnswer {
    Instruction(crate::InstructionSnapshot),
    Schedule(crate::ScheduleSnapshot),
    Wake(crate::WakeSnapshot),
    List {
        automations: Vec<CatalogAutomation>,
        next: Option<String>,
    },
    Mutation {
        automation: CatalogAutomation,
        already_sent: Vec<crate::FiringId>,
    },
    Batches(crate::AutomationBatches),
    Contents(Vec<crate::AutomationContent>),
    Reports(crate::WorkReportAcknowledgements),
    Acknowledged,
    Refused(crate::AutomationRefusal),
}
