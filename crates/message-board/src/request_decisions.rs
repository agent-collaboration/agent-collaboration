//! Terminal ledger decisions are distinct from writer, schema and transport replies.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Assignment, AuthorityRole, EpochNumber, Identity, MoveName, RecordPosition, RecordRef,
    SchemaFingerprint, SubjectRevision, TaskId, WorkstreamId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChangeOutcome {
    Changed,
    AlreadySo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RejectReason {
    StaleRevision {
        current: SubjectRevision,
    },
    MoveNotAllowed {
        edge: MoveName,
        required_role: Option<AuthorityRole>,
    },
    GuardUnmet {
        guard: String,
        evaluator: Identity,
    },
    GuardUnavailable {
        guard: String,
        evaluator: Identity,
    },
    Closed {},
    Archived {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkstreamRefusal {
    RoleRequired { requirement: crate::RoleRequirement },
    PairNotDistinct { holder: Identity },
    Closed { workstream: WorkstreamId },
    NotFound { record: RecordRef },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "details",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TaskRefusal {
    DependencyCycle { path: Vec<TaskId> },
    DependencyMissing(TaskId),
    DependencyDropped(TaskId),
    DependencyCrossesProject(TaskId),
    NotFound { record: RecordRef },
    Closed { subject: RecordRef },
    StaleRevision { current: SubjectRevision },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TrailRefusal {
    SubjectRequired { assignments: Vec<Assignment> },
    NotFound { record: RecordRef },
    InvalidShape { field: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RoleRefusal {
    NoHolder {
        subject: crate::RoleSubject,
        role: AuthorityRole,
    },
    RoleRequired {
        requirement: crate::RoleRequirement,
    },
    PairNotDistinct {
        holder: Identity,
    },
    StandDownUnacknowledged {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "reason",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Refusal {
    Move(RejectReason),
    Workstream(WorkstreamRefusal),
    Task(TaskRefusal),
    Trail(TrailRefusal),
    Role(RoleRefusal),
    Thread(Box<crate::ThreadRefusal>),
    StaleRevision {
        current: SubjectRevision,
    },
    InvalidShape {
        field: String,
    },
    NotFound(RecordRef),
    NameConflict {
        name: crate::ResourceName,
        existing: RecordRef,
    },
    AlreadyExists(RecordRef),
    #[serde(rename = "postCooldown")]
    TopLevelPostCooldown {
        retry_after_seconds: crate::PostCooldownSeconds,
    },
    Closed {
        subject: RecordRef,
    },
    Archived,
    DefinitionConflict,
    ProjectRejected {
        reason: Box<Refusal>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum VoidCause {
    Takeover { epoch: EpochNumber },
    ProjectRejected { reason: Box<Refusal> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RequestDecision {
    Accepted {
        records: Vec<RecordRef>,
        positions: Vec<RecordPosition>,
        change: ChangeOutcome,
    },
    Rejected {
        reasons: Vec<Refusal>,
    },
    Voided {
        cause: VoidCause,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SchemaRefusal {
    TooNew {
        store: u64,
        supported: u64,
    },
    Mismatch {
        version: u64,
        expected: SchemaFingerprint,
        actual: SchemaFingerprint,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "decision",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RequestResult {
    Decided(RequestDecision),
    Unresolved { takeover: EpochNumber },
    ConflictingRequest,
    NotWriter { current: Option<EpochNumber> },
    LeaseExpired { expired_at: DateTime<Utc> },
    SchemaNotReady(SchemaRefusal),
    PendingUnreachable,
    Overloaded,
    ScopeUnavailable(ScopeRefusal),
}

/// Refusals before a writable project scope exists are never ledger decisions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ScopeRefusal {
    Refused {
        reason: Box<Refusal>,
    },
    ProjectNotFound {
        project: crate::ProjectId,
    },
    BoardNotFound {
        board: crate::BoardId,
    },
    ProjectRejected {
        project: crate::ProjectId,
        reason: Box<Refusal>,
    },
}

/// Per-call state never becomes part of a replayable ledger decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderState {
    pub subject: RecordRef,
    pub read_through: crate::FeedCursor,
    pub unread_count: u64,
    pub unread: Vec<RecordRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteReply {
    pub result: RequestResult,
    pub reader: Option<ReaderState>,
}
