//! Machine work is frozen by central and retained until durable local admission is acknowledged.
use crate::{
    AutomationId, BatchId, ContentId, EffectiveProviderSettings, FiringId, GridTime, MailDigest,
    NonEmptyText, ObservationTimestamp, OperationId, RevisionId, SessionRef, WorkItemId,
    limits::{LimitExceeded, LimitName},
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyTaggedVariantFields {}

fn deserialize_empty_tagged_variant<'de, Deserializer>(
    deserializer: Deserializer,
) -> Result<(), Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    // Serde's default internally tagged unit visitor ignores remaining fields.
    EmptyTaggedVariantFields::deserialize(deserializer)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct WorkReportSequence(u64);
impl TryFrom<u64> for WorkReportSequence {
    type Error = LimitExceeded;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        LimitName::JsonSafeInteger.validate(value)?;
        Ok(Self(value))
    }
}
impl From<WorkReportSequence> for u64 {
    fn from(value: WorkReportSequence) -> Self {
        value.0
    }
}
impl JsonSchema for WorkReportSequence {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "WorkReportSequence".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        LimitName::JsonSafeInteger.definition().schema()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentReference {
    pub content_id: ContentId,
    pub revision_id: RevisionId,
    pub digest: MailDigest,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FiringWork {
    Wake {
        created_by: crate::WakeCreator,
        #[schemars(schema_with = "crate::automation_timing_contract::wake_timing_schema")]
        timing: crate::TimingRequest,
        anchor_at: ObservationTimestamp,
        created_at: ObservationTimestamp,
        target: SessionRef,
        origin: FiringMessageOrigin,
        delivery: crate::MessageDelivery,
        generation_guard: Option<crate::CodexGeneration>,
        expires_at: ObservationTimestamp,
    },
    Schedule {
        #[schemars(schema_with = "frozen_schedule_schema")]
        definition: crate::ScheduleDefinition,
    },
}

pub(crate) fn frozen_schedule_schema(generator: &mut SchemaGenerator) -> Schema {
    let mut schema = crate::ScheduleDefinition::json_schema(generator);
    let timing = crate::automation_timing_contract::central_timing_schema(generator).to_value();
    let effort = generator.subschema_for::<Option<String>>().to_value();
    if let Some(properties) = schema
        .as_object_mut()
        .and_then(|root| root.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
    {
        properties.insert("timing".into(), timing);
        // A frozen record preserves its nullable choice; the mutation schema
        // separately requires an explicit effort selection.
        properties.insert("effort".into(), effort);
    }
    schema
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FiringMessageOrigin {
    Agent {
        sender: SessionRef,
    },
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    HumanUser,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Router,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FiringEntry {
    pub firing_id: FiringId,
    pub automation_id: AutomationId,
    pub due_at: GridTime,
    pub revision: crate::ChangeId,
    pub content: ContentReference,
    pub work: FiringWork,
}

#[derive(Deserialize)]
#[serde(remote = "FiringEntry", rename_all = "camelCase", deny_unknown_fields)]
struct FiringEntryFields {
    firing_id: FiringId,
    automation_id: AutomationId,
    due_at: GridTime,
    revision: crate::ChangeId,
    content: ContentReference,
    work: FiringWork,
}

impl<'de> Deserialize<'de> for FiringEntry {
    fn deserialize<Deserializer: serde::Deserializer<'de>>(
        deserializer: Deserializer,
    ) -> Result<Self, Deserializer::Error> {
        let entry = FiringEntryFields::deserialize(deserializer)?;
        let (timing, kind, now_ms) = match &entry.work {
            FiringWork::Schedule { definition } => (
                &definition.timing,
                crate::AutomationKind::Schedule,
                entry.due_at.unix_millis(),
            ),
            FiringWork::Wake {
                timing, anchor_at, ..
            } => (
                timing,
                crate::AutomationKind::Wake,
                String::from(anchor_at.clone())
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .map_err(serde::de::Error::custom)?
                    .timestamp_millis(),
            ),
        };
        crate::AutomationTiming::resolve(timing, kind, now_ms).map_err(serde::de::Error::custom)?;
        Ok(entry)
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionCreateEntry {
    pub work_item_id: WorkItemId,
    pub operation_id: OperationId,
    pub source_installation: message_board::ServiceId,
    pub request: RemoteSessionCreateDefinition,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteSessionCreateDefinition {
    pub endpoint: crate::EndpointRef,
    pub working_directory: crate::ProviderWorkingDirectory,
    pub created_by: crate::ProviderIdentity,
    pub approver: Option<crate::ProviderIdentity>,
    pub requested_policy: crate::ProviderRequestedPolicy,
    pub settings: Option<crate::ProviderRequestedSettings>,
    pub first_input: Option<crate::MessageContent>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteSessionCreateRequest {
    pub operation_id: OperationId,
    pub definition: RemoteSessionCreateDefinition,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkEntry {
    Firing(FiringEntry),
    SessionCreate(SessionCreateEntry),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "Vec<WorkEntry>", into = "Vec<WorkEntry>")]
pub struct BatchEntries(Vec<WorkEntry>);
impl BatchEntries {
    #[must_use]
    pub fn entries(&self) -> &[WorkEntry] {
        &self.0
    }
}
impl TryFrom<Vec<WorkEntry>> for BatchEntries {
    type Error = LimitExceeded;
    fn try_from(entries: Vec<WorkEntry>) -> Result<Self, Self::Error> {
        LimitName::BatchMaxEntries.validate(entries.len() as u64)?;
        Ok(Self(entries))
    }
}
impl From<BatchEntries> for Vec<WorkEntry> {
    fn from(entries: BatchEntries) -> Self {
        entries.0
    }
}
impl JsonSchema for BatchEntries {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BatchEntries".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"array", "minItems":1, "maxItems":crate::limits::BATCH_MAX_ENTRIES,
            "items":generator.subschema_for::<WorkEntry>(),
            "description":LimitName::BatchMaxEntries.definition().description})
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationBatch {
    pub batch_id: BatchId,
    pub service_id: crate::UuidIdentity,
    pub entries: BatchEntries,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FiringOutcome {
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Pending,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Cancelled,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Held,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Received,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Delivered,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Expired,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Unknown,
    RunStarted {
        run_id: crate::RunId,
    },
    RunFinished {
        run_id: crate::RunId,
        summary: Option<crate::InstructionText>,
    },
    Failed {
        reason: NonEmptyText,
    },
    Uncertain {
        reason: NonEmptyText,
    },
}
impl FiringOutcome {
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        match self {
            Self::Cancelled
            | Self::Delivered
            | Self::Expired
            | Self::RunFinished { .. }
            | Self::Failed { .. } => true,
            Self::Pending
            | Self::Held
            | Self::Received
            | Self::Unknown
            | Self::RunStarted { .. }
            | Self::Uncertain { .. } => false,
        }
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FiringReport {
    pub firing_id: FiringId,
    pub seq: WorkReportSequence,
    pub outcome: FiringOutcome,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionCreateState {
    Created {
        session_ref: SessionRef,
        settings: RemoteCreatedSettings,
    },
    Refused {
        reason: RemoteCreateRefusal,
    },
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Unknown,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteCreatedSettings {
    Applied {
        effective_settings: Option<EffectiveProviderSettings>,
        permission_differences: Vec<crate::PermissionDifference>,
    },
    PartlyApplied {
        applied: Vec<crate::AppliedProviderSetting>,
        failed: Vec<crate::FailedProviderSetting>,
        not_applied: Vec<crate::NotAppliedProviderSetting>,
    },
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteCreateRefusal {
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    OriginMismatch,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    CreatorNotSupported,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    ApproverRequired,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    ApproverNotSupported,
    Local {
        failure: Box<crate::ConversationOperationFailure>,
    },
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum FirstInputState {
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    None,
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Accepted,
    Failed {
        reason: NonEmptyText,
    },
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Unknown,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkItemReport {
    Firing(FiringReport),
    SessionCreate {
        work_item_id: WorkItemId,
        seq: WorkReportSequence,
        create: Box<SessionCreateState>,
        input: FirstInputState,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MachineWorkProtocol {
    MachineWorkV1,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullAutomationBatches {
    pub after: Option<BatchId>,
    pub max_bytes: u32,
    pub work_protocols: Vec<MachineWorkProtocol>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenAutomationBatch {
    pub batch_id: BatchId,
    pub digest: MailDigest,
    pub frozen_bytes: Vec<u8>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationBatches {
    pub batches: Vec<FrozenAutomationBatch>,
    pub next: Option<BatchId>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FetchContents {
    pub contents: Vec<ContentReference>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationContent {
    pub reference: ContentReference,
    pub text: crate::InstructionText,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchAcknowledgement {
    pub batch_id: BatchId,
    pub digest: MailDigest,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AckAutomationBatches {
    pub batches: Vec<BatchAcknowledgement>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "ReportFields")]
pub struct ReportWorkItems {
    reports: Vec<WorkItemReport>,
}

#[derive(JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReportFields {
    #[schemars(length(min = 1, max = "crate::limits::REPORT_FLUSH_MAX"))]
    reports: Vec<WorkItemReport>,
}
impl ReportWorkItems {
    pub fn new(reports: Vec<WorkItemReport>) -> Result<Self, LimitExceeded> {
        LimitName::ReportFlushMax.validate(reports.len() as u64)?;
        Ok(Self { reports })
    }
    #[must_use]
    pub fn reports(&self) -> &[WorkItemReport] {
        &self.reports
    }
}
impl TryFrom<ReportFields> for ReportWorkItems {
    type Error = LimitExceeded;
    fn try_from(fields: ReportFields) -> Result<Self, Self::Error> {
        Self::new(fields.reports)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkReportSubject {
    Firing { firing_id: FiringId },
    SessionCreate { work_item_id: WorkItemId },
}

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptedWorkReport {
    pub subject: WorkReportSubject,
    pub seq: WorkReportSequence,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkReportAcknowledgements {
    pub accepted: Vec<AcceptedWorkReport>,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum LocalRunOutcome {
    #[serde(deserialize_with = "deserialize_empty_tagged_variant")]
    Finished,
    Failed {
        reason: NonEmptyText,
    },
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalRunSettled {
    pub automation_id: AutomationId,
    pub run_id: crate::RunId,
    pub outcome: LocalRunOutcome,
}
