//! Feed coverage remains explicit even when content is redacted or filtered away.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AssignmentRevision, AuthorityRole, BoardId, CanonicalHash, EpochNumber, EvidencePointer,
    FeedCursor, FeedPosition, Identity, NotifyIntentId, PlanRevisionHash, ProjectId, RecordId,
    RecordPosition, RecordRef, RecordedActor, RequestDecision, RequestId, RoleSubject, ServiceId,
    SubjectRevision, WorkTrailRow, WorkspaceId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordScope {
    pub workspace: WorkspaceId,
    pub board: Option<BoardId>,
    pub project: Option<ProjectId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordEnvelope {
    pub id: RecordId,
    pub scope: RecordScope,
    pub actor: RecordedActor,
    pub request: Option<RequestId>,
    pub position: RecordPosition,
    pub router_time: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "record",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Record {
    Workstream(crate::Workstream),
    Task(crate::Task),
    TaskLinkEvent(crate::TaskLinkEvent),
    Subtask(crate::Subtask),
    WorkTrailRow(WorkTrailRow),
    TrailRedaction(crate::TrailRedaction),
    RoleAssignment(crate::RoleAssignment),
    OwnerGrant(crate::OwnerGrant),
    StandDownAcknowledgement(crate::StandDownAcknowledgement),
    Move(crate::MoveRecord),
    Fact(crate::Fact),
    Project(crate::ProjectState),
    PlanRevision(PlanRevision),
    ReadMark(ReadMark),
    NotifyIntent(NotifyIntent),
    SubscriptionMembership(crate::SubscriptionMembership),
    Request(RequestLedgerEntry),
    Post(crate::Post),
    Thread(crate::Thread),
    ThreadRefiled(crate::ThreadRefiled),
    Workspace(crate::Workspace),
    Board(crate::Board),
    Adr(crate::Adr),
    ProcessDefinition(crate::ProcessDefinition),
    DirectoryEntry(crate::ProjectDirectoryEntry),
}

impl Record {
    #[must_use]
    pub fn envelope(&self) -> &RecordEnvelope {
        match self {
            Self::Workstream(row) => &row.envelope,
            Self::Task(row) => &row.envelope,
            Self::TaskLinkEvent(row) => &row.envelope,
            Self::Subtask(row) => &row.envelope,
            Self::WorkTrailRow(row) => &row.envelope,
            Self::TrailRedaction(row) => &row.envelope,
            Self::RoleAssignment(row) => &row.envelope,
            Self::OwnerGrant(row) => &row.envelope,
            Self::StandDownAcknowledgement(row) => &row.envelope,
            Self::Move(row) => &row.envelope,
            Self::Fact(row) => &row.envelope,
            Self::Project(row) => &row.envelope,
            Self::PlanRevision(row) => &row.envelope,
            Self::ReadMark(row) => &row.envelope,
            Self::NotifyIntent(row) => &row.envelope,
            Self::SubscriptionMembership(row) => &row.envelope,
            Self::Request(row) => &row.envelope,
            Self::Post(row) => &row.envelope,
            Self::Thread(row) => &row.envelope,
            Self::ThreadRefiled(row) => &row.envelope,
            Self::Workspace(row) => &row.envelope,
            Self::Board(row) => &row.envelope,
            Self::Adr(row) => &row.envelope,
            Self::ProcessDefinition(row) => &row.envelope,
            Self::DirectoryEntry(row) => &row.envelope,
        }
    }
    pub fn envelope_mut(&mut self) -> &mut RecordEnvelope {
        match self {
            Self::Workstream(row) => &mut row.envelope,
            Self::Task(row) => &mut row.envelope,
            Self::TaskLinkEvent(row) => &mut row.envelope,
            Self::Subtask(row) => &mut row.envelope,
            Self::WorkTrailRow(row) => &mut row.envelope,
            Self::TrailRedaction(row) => &mut row.envelope,
            Self::RoleAssignment(row) => &mut row.envelope,
            Self::OwnerGrant(row) => &mut row.envelope,
            Self::StandDownAcknowledgement(row) => &mut row.envelope,
            Self::Move(row) => &mut row.envelope,
            Self::Fact(row) => &mut row.envelope,
            Self::Project(row) => &mut row.envelope,
            Self::PlanRevision(row) => &mut row.envelope,
            Self::ReadMark(row) => &mut row.envelope,
            Self::NotifyIntent(row) => &mut row.envelope,
            Self::SubscriptionMembership(row) => &mut row.envelope,
            Self::Request(row) => &mut row.envelope,
            Self::Post(row) => &mut row.envelope,
            Self::Thread(row) => &mut row.envelope,
            Self::ThreadRefiled(row) => &mut row.envelope,
            Self::Workspace(row) => &mut row.envelope,
            Self::Board(row) => &mut row.envelope,
            Self::Adr(row) => &mut row.envelope,
            Self::ProcessDefinition(row) => &mut row.envelope,
            Self::DirectoryEntry(row) => &mut row.envelope,
        }
    }
    #[must_use]
    pub fn reference(&self) -> RecordRef {
        match self {
            Self::Workstream(row) => row.workstream_id.clone().into(),
            Self::Task(row) => row.task_id.clone().into(),
            Self::TaskLinkEvent(row) => row.id.clone().into(),
            Self::Subtask(row) => row.subtask_id.clone().into(),
            Self::WorkTrailRow(row) => row.row_id.clone().into(),
            Self::RoleAssignment(row) => row.assignment_id.clone().into(),
            Self::OwnerGrant(row) => row.id.clone().into(),
            Self::StandDownAcknowledgement(_) => RecordRef {
                kind: crate::RecordKind::StandDownAcknowledgement,
                id: self.envelope().id.clone(),
            },
            Self::Move(row) => row.move_id.clone().into(),
            Self::Fact(row) => row.fact_id.clone().into(),
            Self::Project(row) => row.project_id.clone().into(),
            Self::NotifyIntent(row) => row.intent_id.clone().into(),
            Self::SubscriptionMembership(_) => RecordRef {
                kind: crate::RecordKind::SubscriptionMembership,
                id: self.envelope().id.clone(),
            },
            Self::Request(row) => row.request_id.clone().into(),
            Self::Post(row) => row.post_id.clone().into(),
            Self::Thread(row) => row.thread_id.clone().into(),
            Self::ThreadRefiled(row) => RecordRef {
                kind: crate::RecordKind::ThreadRefiled,
                id: row.envelope.id.clone(),
            },
            Self::Workspace(row) => row.workspace_id.clone().into(),
            Self::Board(row) => row.board_id.clone().into(),
            Self::Adr(row) => row.adr_id.clone().into(),
            Self::DirectoryEntry(row) => row.id.clone().into(),
            Self::TrailRedaction(_) => RecordRef {
                kind: crate::RecordKind::TrailRedaction,
                id: self.envelope().id.clone(),
            },
            Self::PlanRevision(_) => RecordRef {
                kind: crate::RecordKind::PlanRevision,
                id: self.envelope().id.clone(),
            },
            Self::ReadMark(_) => RecordRef {
                kind: crate::RecordKind::ReadMark,
                id: self.envelope().id.clone(),
            },
            Self::ProcessDefinition(_) => RecordRef {
                kind: crate::RecordKind::ProcessDefinition,
                id: self.envelope().id.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "record",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FeedContent {
    Shown(Box<Record>),
    Redacted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedEntry {
    pub position: FeedPosition,
    pub epoch: EpochNumber,
    pub record: RecordRef,
    pub request: Option<RequestId>,
    pub router_time: DateTime<Utc>,
    pub actor: RecordedActor,
    pub content: FeedContent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FeedRead {
    Entries {
        entries: Vec<FeedEntry>,
        covered_through: FeedCursor,
        high_water: FeedCursor,
    },
    CursorSuperseded {
        resume_from: FeedCursor,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrailPage {
    pub rows: Vec<WorkTrailRow>,
    pub covered_through: FeedCursor,
    pub high_water: FeedCursor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionPage {
    pub rows: Vec<WorkTrailRow>,
    pub older_than: Option<FeedCursor>,
    pub range_top: FeedCursor,
    pub covered_through: Option<FeedCursor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadMark {
    pub envelope: crate::RecordEnvelope,
    pub reader: Identity,
    pub scope: crate::ReadScope,
    pub through: FeedCursor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestLedgerEntry {
    pub envelope: crate::RecordEnvelope,
    pub request_id: RequestId,
    pub canonical_hash: CanonicalHash,
    pub observed_epoch: Option<EpochNumber>,
    pub accepted_in: Option<EpochNumber>,
    pub decision: RequestDecision,
    pub resubmits: Option<RequestId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EpochEnd {
    Released {},
    Superseded {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriterEpoch {
    pub project: ProjectId,
    pub number: EpochNumber,
    pub holder: ServiceId,
    pub start_position: FeedPosition,
    pub end: Option<EpochEnd>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRevision {
    pub envelope: crate::RecordEnvelope,
    pub hash: PlanRevisionHash,
    pub document: EvidencePointer,
    pub superseded_by: Option<PlanRevisionHash>,
    pub revision: SubjectRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum IntentState {
    Emitted {},
    HandedOff {},
    Superseded {},
    Refused { reason: IntentRefusal },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum IntentRefusal {
    NoHolder {},
    Delivery { detail: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoleRecipient {
    pub subject: RoleSubject,
    pub role: AuthorityRole,
    pub assignment_revision: AssignmentRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedRecipient {
    pub actor: RecordedActor,
    pub via_role: Option<RoleRecipient>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "recipient",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum IntentRecipient {
    Resolved(ResolvedRecipient),
    NoHolder {
        subject: RoleSubject,
        role: AuthorityRole,
        assignment_revision: AssignmentRevision,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "NotifyIntentProps", into = "NotifyIntentProps")]
pub struct NotifyIntent {
    pub envelope: crate::RecordEnvelope,
    pub intent_id: NotifyIntentId,
    pub cause: RecordRef,
    recipient: IntentRecipient,
    state: IntentState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotifyIntentProps {
    pub envelope: crate::RecordEnvelope,
    pub intent_id: NotifyIntentId,
    pub cause: RecordRef,
    pub recipient: IntentRecipient,
    pub state: IntentState,
}

fn validate_intent_state(
    recipient: &IntentRecipient,
    state: &IntentState,
) -> Result<(), crate::TextValidationError> {
    if let IntentRecipient::Resolved(recipient) = recipient
        && recipient.actor.declared_identity().is_none()
    {
        return Err(crate::TextValidationError {
            field: "recipient",
            requirement: "must resolve to a declared identity",
        });
    }
    let valid = matches!(
        (recipient, state),
        (
            IntentRecipient::NoHolder { .. },
            IntentState::Refused {
                reason: IntentRefusal::NoHolder {}
            }
        ) | (
            IntentRecipient::Resolved(_),
            IntentState::Emitted {}
                | IntentState::HandedOff {}
                | IntentState::Superseded {}
                | IntentState::Refused {
                    reason: IntentRefusal::Delivery { .. }
                }
        )
    );
    if !valid {
        return Err(crate::TextValidationError {
            field: "state",
            requirement: "must agree with the recipient resolution",
        });
    }
    let role = match recipient {
        IntentRecipient::NoHolder { subject, role, .. } => Some((subject, role)),
        IntentRecipient::Resolved(recipient) => recipient
            .via_role
            .as_ref()
            .map(|resolution| (&resolution.subject, &resolution.role)),
    };
    if let Some((subject, role)) = role
        && !matches!(
            (subject, role),
            (
                RoleSubject::Project(_),
                AuthorityRole::Lead | AuthorityRole::Advisor
            ) | (
                RoleSubject::Workstream(_),
                AuthorityRole::Implementer
                    | AuthorityRole::Verifier
                    | AuthorityRole::Reviewer
                    | AuthorityRole::Operator
            )
        )
    {
        return Err(crate::TextValidationError {
            field: "recipient",
            requirement: "must name a role on its valid subject",
        });
    }
    Ok(())
}

impl NotifyIntent {
    pub fn new(props: NotifyIntentProps) -> Result<Self, crate::TextValidationError> {
        validate_intent_state(&props.recipient, &props.state)?;
        Ok(Self {
            envelope: props.envelope,
            intent_id: props.intent_id,
            cause: props.cause,
            recipient: props.recipient,
            state: props.state,
        })
    }
    #[must_use]
    pub fn recipient(&self) -> &IntentRecipient {
        &self.recipient
    }
    #[must_use]
    pub fn state(&self) -> &IntentState {
        &self.state
    }
    pub fn set_state(&mut self, state: IntentState) -> Result<(), crate::TextValidationError> {
        validate_intent_state(&self.recipient, &state)?;
        self.state = state;
        Ok(())
    }
}
impl TryFrom<NotifyIntentProps> for NotifyIntent {
    type Error = crate::TextValidationError;
    fn try_from(props: NotifyIntentProps) -> Result<Self, Self::Error> {
        Self::new(props)
    }
}
impl From<NotifyIntent> for NotifyIntentProps {
    fn from(intent: NotifyIntent) -> Self {
        Self {
            envelope: intent.envelope,
            intent_id: intent.intent_id,
            cause: intent.cause,
            recipient: intent.recipient,
            state: intent.state,
        }
    }
}
