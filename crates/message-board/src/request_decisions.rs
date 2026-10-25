//! Terminal ledger decisions are distinct from writer, schema and transport replies.

use crate::ProjectId;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Assignment, AuthorityRole, EpochNumber, Identity, RecordRef, SchemaFingerprint,
    SubjectRevision, TaskId, WorkstreamId,
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
    /// When abridged, the path is a prefix of the closed witness, not a closed cycle.
    DependencyCycle {
        path: crate::AbridgedEvidence<TaskId>,
    },
    DependencyMissing(TaskId),
    DependencyDropped(TaskId),
    DependencyCrossesProject(TaskId),
    NotFound {
        record: RecordRef,
    },
    Closed {
        subject: RecordRef,
    },
    StaleRevision {
        current: SubjectRevision,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TrailRefusal {
    /// When abridged, entries are a non-exhaustive eligible-assignment prefix.
    SubjectRequired {
        assignments: crate::AbridgedEvidence<Assignment>,
    },
    NotFound {
        record: RecordRef,
    },
    InvalidShape {
        field: String,
    },
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
    ProjectRejected {
        reason: Box<Refusal>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRegistrationRejection {
    pub code: String,
    pub detail: String,
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
        effects: crate::DecisionEffectsPage,
        change: ChangeOutcome,
    },
    Rejected {
        reasons: Vec<Refusal>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StoredDecision {
    Accepted {
        effects: crate::DecisionEffectsHeader,
        change: ChangeOutcome,
    },
    Rejected {
        reasons: Vec<Refusal>,
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
    ConflictingRequest,
    NotWriter { current: Option<EpochNumber> },
    LeaseExpired { expired_at: DateTime<Utc> },
    TransferInProgress { transfer: crate::TransferId },
    SchemaNotReady(SchemaRefusal),
    PendingUnreachable,
    ProjectCreationPending { project: ProjectId },
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
        reason: ProjectRejection,
    },
}

/// Preserve the refusing owner's typed answer before a project can accept writes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProjectRejection {
    Directory {
        refusal: Refusal,
    },
    Registration {
        reason: ProjectRegistrationRejection,
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
