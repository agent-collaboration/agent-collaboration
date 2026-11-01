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
    FiringShow {
        #[serde(rename = "firingId")]
        firing_id: crate::FiringId,
    },
    FiringList(FiringListRequest),
    PullAutomationBatches(crate::PullAutomationBatches),
    FetchContents(crate::FetchContents),
    AckAutomationBatches(crate::AckAutomationBatches),
    ReportWorkItems(crate::ReportWorkItems),
    LocalRunSettled(crate::LocalRunSettled),
    RemoteSessionCreate(crate::RemoteSessionCreateRequest),
    RemoteSessionCreateStatus {
        #[serde(rename = "operationId")]
        operation_id: crate::OperationId,
    },
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
    Wake(CentralWakeSnapshot),
    Faulted(crate::FaultedAutomationSnapshot),
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("allOf" = [{"required":["nextDueAt","firstFiring"]}]))]
pub struct CentralWakeSnapshot {
    pub definition: crate::WakeDefinition,
    pub state: crate::WakeState,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next_due_at: Option<crate::ObservationTimestamp>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub first_firing: Option<FiringSnapshot>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FiringListRequest {
    pub automation_id: crate::AutomationId,
    #[serde(default, deserialize_with = "Option::deserialize")]
    pub cursor: Option<String>,
    pub limit: crate::PageLimit,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("allOf" = [{"required":["reportSeq","batchId"]}]))]
pub struct FiringSnapshot {
    pub firing_id: crate::FiringId,
    pub automation_id: crate::AutomationId,
    pub due_at: crate::GridTime,
    pub revision: crate::ChangeId,
    pub content: crate::ContentReference,
    pub outcome: crate::FiringOutcome,
    /// Null until the first machine report; otherwise its highest accepted sequence.
    #[serde(deserialize_with = "Option::deserialize")]
    pub report_seq: Option<crate::WorkReportSequence>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub batch_id: Option<crate::BatchId>,
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
    Instructions(crate::AutomationPage<crate::InstructionSnapshot>),
    Revisions(crate::AutomationPage<crate::RevisionRecord>),
    Schedule(crate::ScheduleSnapshot),
    Wake(CentralWakeSnapshot),
    Firing(FiringSnapshot),
    SessionCreate(crate::RemoteSessionCreateSnapshot),
    Faulted(crate::FaultedAutomationSnapshot),
    Firings(crate::AutomationPage<FiringSnapshot>),
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
