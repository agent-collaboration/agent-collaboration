//! Workstreams carry a pair; tasks and checklist items remain lightweight.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    EvidencePointer, ProjectId, ResourceName, SubjectRevision, SubtaskId, SubtaskPlace, TaskId,
    TaskTitle, TrailTag, TrailText, WorkTrailRowId, WorkstreamId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CloseReason {
    Done,
    Dropped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkstreamState {
    Open {},
    Closed { reason: CloseReason, note: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workstream {
    pub envelope: crate::RecordEnvelope,
    pub workstream_id: WorkstreamId,
    pub project_id: ProjectId,
    pub name: ResourceName,
    pub goal: String,
    pub state: WorkstreamState,
    pub revision: SubjectRevision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
    Dropped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TaskStatusChange {
    Todo,
    Doing,
    Done,
    Drop { reason: crate::DropReason },
}
impl TaskStatusChange {
    #[must_use]
    pub fn target_status(&self) -> TaskStatus {
        match self {
            Self::Todo => TaskStatus::Todo,
            Self::Doing => TaskStatus::Doing,
            Self::Done => TaskStatus::Done,
            Self::Drop { .. } => TaskStatus::Dropped,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Task {
    pub envelope: crate::RecordEnvelope,
    pub task_id: TaskId,
    pub project_id: ProjectId,
    pub workstream_id: WorkstreamId,
    pub title: TaskTitle,
    pub body: String,
    pub status: TaskStatus,
    pub revision: SubjectRevision,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum TaskLinkKind {
    BlockedBy,
    RelatedTo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskLink {
    pub from: TaskId,
    pub kind: TaskLinkKind,
    pub to: TaskId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "link",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum TaskLinkEventKind {
    Added(TaskLink),
    Removed(TaskLink),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskLinkEvent {
    pub envelope: crate::RecordEnvelope,
    pub id: crate::TaskLinkEventId,
    pub change: TaskLinkEventKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Subtask {
    pub envelope: crate::RecordEnvelope,
    pub subtask_id: SubtaskId,
    pub task_id: TaskId,
    pub text: TrailText,
    pub checked: bool,
    pub removed: bool,
    pub place: SubtaskPlace,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "subject",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum TrailSubject {
    Project,
    Workstream(WorkstreamId),
    Task(TaskId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkTrailRow {
    pub envelope: crate::RecordEnvelope,
    pub row_id: WorkTrailRowId,
    pub project_id: ProjectId,
    pub subject: TrailSubject,
    pub text: TrailText,
    pub tags: Vec<TrailTag>,
    pub links: Vec<EvidencePointer>,
    pub redacted: bool,
}

impl WorkTrailRow {
    #[must_use]
    pub fn is_decision(&self) -> bool {
        self.tags.iter().any(|tag| tag.as_str() == "decision")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrailRedaction {
    pub envelope: crate::RecordEnvelope,
    pub row: WorkTrailRowId,
    pub redacted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TaskAction {
    Duplicate { of: TaskId, note: String },
    Replace { by: TaskId, note: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LinkChange {
    Added,
    AlreadyPresent,
    Removed,
    NotPresent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RedactionChange {
    Changed,
    AlreadySo,
}
