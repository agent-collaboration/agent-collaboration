//! Views expose snapshot and replication positions; none has a write operation.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Assignment, EpochNumber, FactId, FeedCursor, PlanRevisionHash, ProjectId, RecordKind,
    RecordRef, RecordedActor, ResourceName, SessionRef, SubjectRevision, TaskId, TaskStatus,
    TaskTitle, WorkTrailRow, WorkstreamId, WorkstreamState,
};

macro_rules! view {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),* }
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CopySource {
    Writer {},
    ReadCopy {},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CurrentWriter {
    Held { epoch: EpochNumber },
    Unheld {},
    Unknown {},
}

view!(ContinuationCard {
    subject: RecordRef, plan_revision: Option<PlanRevisionHash>,
    assignment: Option<Assignment>,
    pending_handbacks: Vec<RecordRef>, pending_decisions: Vec<RecordRef>,
    next_obligation: Option<LeadOwed>, recent_trail: Vec<WorkTrailRow>,
    counts: BTreeMap<RecordKind, u64>, as_of: FeedCursor, replicated_through: FeedCursor,
    source: CopySource, silence_rule: String,
    notify_intents: Vec<crate::NotifyIntent>, blockers: Vec<BlockedTask>,
    session_state: SessionRunningState,
});

/// Only the interactions/Host presence port may answer a session's running state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionRunningState {
    Running {},
    Wakeable {},
    Unavailable { reason: String },
    NotSession {},
}
view!(WorkView {
    project: ProjectId, as_of: FeedCursor, replicated_through: FeedCursor,
    workstreams: Vec<WorkstreamLane>, ready_queue: Vec<TaskId>, blocked: Vec<BlockedTask>,
    open_in_closed_workstreams: Vec<TaskId>,
    current_writer: CurrentWriter, source: CopySource,
    writer_observation: Option<crate::WriterObservation>,
});
view!(WorkstreamLane { id: WorkstreamId, name: ResourceName, state: WorkstreamState,
    implementer: Option<RecordedActor>, verifier: Option<RecordedActor>, tasks: Vec<WorkTask> });
view!(WorkTask {
    id: TaskId,
    title: TaskTitle,
    status: TaskStatus,
    subtasks_checked: u32,
    subtasks_total: u32,
    revision: SubjectRevision
});
view!(BlockedTask { task: TaskId, blockers: Vec<TaskId>, by_dropped: bool });
view!(OwnerView { as_of: DateTime<Utc>, waiting: Vec<OwnerWait>, unavailable_projects: Vec<ProjectId> });
view!(OwnerWait { project: ProjectId, subject: RecordRef, blocks: Vec<RecordRef>, since: FeedCursor,
    source: CopySource, as_of: FeedCursor, replicated_through: FeedCursor });
view!(LeadLoad { lead: RecordedActor, owed: Vec<LeadOwed>, unavailable_projects: Vec<ProjectId> });
view!(LeadOwed {
    project: ProjectId,
    subject: RecordRef,
    obligation: LeadObligation,
    source: CopySource,
    as_of: FeedCursor,
    replicated_through: FeedCursor
});

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LeadObligation {
    Assess,
    CloseOut,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum UnavailableReason {
    BoardUnreachable {},
    SessionMetadataUnavailable { session: SessionRef },
    StoreUnavailable { project: ProjectId },
    SchemaNotReady { project: ProjectId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionContext {
    Context {
        session: SessionRef,
        assignments: Vec<Assignment>,
        acknowledged_scopes: Vec<FactId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum IntentClaim {
    Claimed {
        recipient: crate::ResolvedRecipient,
        replacements: Vec<crate::NotifyIntentId>,
    },
    Superseded {
        replacements: Vec<crate::NotifyIntentId>,
    },
    Deferred {
        recipient: crate::ResolvedRecipient,
        replacements: Vec<crate::NotifyIntentId>,
    },
}
