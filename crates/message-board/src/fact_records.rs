//! Facts retain producer time and causal anchors separately from accepting Router metadata.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    BoardId, CanonicalHash, CommitSha, FactId, FeedCursor, MoveId, NormalizedOrigin, PostId,
    RecordRef, RecordedActor, RequestId, SessionRef, TaskId, TrailSubject, WorkstreamId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "reference",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EvidenceRef {
    Post(PostId),
    Fact(FactId),
    PrFact(FactId),
    Artifact {
        repository: NormalizedOrigin,
        path: String,
        commit: CommitSha,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactRef {
    pub repository: NormalizedOrigin,
    pub path: String,
    pub commit: Option<CommitSha>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandRun {
    pub command: String,
    pub exit: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReceiptOutcome {
    Verified,
    NotVerified,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReceiptFact {
    pub task: TaskId,
    pub obligation: String,
    pub head: CommitSha,
    pub tree: Option<CommitSha>,
    pub contract_revision: CanonicalHash,
    pub verifier: RecordedActor,
    pub scenario_run: String,
    pub observed: String,
    pub outcome: ReceiptOutcome,
    pub commands: Vec<CommandRun>,
    pub artifacts: Vec<ArtifactRef>,
    pub commission: Option<MoveId>,
    pub executor: Option<SessionRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluatorRef {
    pub actor: RecordedActor,
    pub engine: Option<String>,
    pub calibration: Option<CanonicalHash>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PrFactKind {
    BaseRecorded { base: CommitSha },
    HeadObserved { head: CommitSha },
    Merged { merge_commit: CommitSha },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HookKind {
    SessionStart,
    PreEdit,
    PostShell,
    PreCommit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LifecycleFact {
    ProjectCreated {
        entry: Box<crate::ProjectDirectoryEntry>,
        creating_request: RequestId,
        acting_for: Option<crate::ActingForIdentity>,
    },
    ThreadResolved {
        thread: crate::ThreadId,
        reason: crate::ThreadResolutionReason,
    },
    Closed {
        position: FeedCursor,
        reason: String,
    },
    WorkstreamClosed {
        workstream: WorkstreamId,
        reason: crate::CloseReason,
    },
    Dropped {
        task: TaskId,
        reason: String,
    },
    PhaseCompleted {
        phase: String,
    },
    ArchiveSeen {
        board: BoardId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "fact",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FactKind {
    Receipt(Box<ReceiptFact>),
    EvaluatorResult {
        guard: String,
        subject: RecordRef,
        outcome: crate::GuardOutcome,
        evidence: Vec<EvidenceRef>,
        evaluator: EvaluatorRef,
        executor: Option<SessionRef>,
    },
    PrFact {
        repository: NormalizedOrigin,
        number: u64,
        kind: PrFactKind,
        recorded_by: RecordedActor,
    },
    HookFact {
        hook: HookKind,
        session: SessionRef,
        targets: Vec<String>,
    },
    ScopeAcknowledgement {
        session: SessionRef,
        subject: TrailSubject,
        paths: Vec<String>,
        reason: String,
    },
    Lifecycle(LifecycleFact),
    // G3 is deliberately unresolved; admission does not accept this deferred kind.
    OwnerWords,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fact {
    pub envelope: crate::RecordEnvelope,
    pub fact_id: FactId,
    pub kind: FactKind,
    pub links: Vec<RecordRef>,
    pub producer_time: DateTime<Utc>,
    pub anchor: Option<FeedCursor>,
    pub late: bool,
}

/// Producers supply evidence and causal time. Admission assigns the envelope and late flag.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FactInput {
    pub fact_id: FactId,
    pub kind: FactKind,
    pub links: Vec<RecordRef>,
    pub producer_time: DateTime<Utc>,
    pub anchor: Option<FeedCursor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GuardOutcome {
    Met,
    Unmet,
    Unavailable,
}
