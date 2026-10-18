//! Common record-read envelopes keep project identity explicit at the API boundary.
use crate::{PageRequest, ProjectId, RecordKind, RecordRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectMoveCheckRequest {
    pub project_id: ProjectId,
    pub actor: crate::Identity,
    pub move_name: crate::MoveName,
    pub expected_revision: crate::SubjectRevision,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRecordShowRequest {
    pub project_id: ProjectId,
    pub record: RecordRef,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRecordListRequest {
    pub project_id: ProjectId,
    pub page: PageRequest,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtaskListRequest {
    pub project_id: ProjectId,
    pub page: PageRequest,
    #[serde(default)]
    pub include_removed: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetRecordShowRequest {
    pub record: RecordRef,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetRecordListRequest {
    pub workspace_id: crate::WorkspaceId,
    pub page: PageRequest,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedReadRequest {
    pub reader: crate::ReadActor,
    pub project_id: ProjectId,
    pub after: crate::FeedCursor,
    pub limit: crate::PageLimit,
    pub include_redacted: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkTrailShowRequest {
    pub reader: crate::ReadActor,
    pub project_id: ProjectId,
    pub selection: WorkTrailSelection,
    pub limit: crate::PageLimit,
    pub subject: Option<crate::TrailSubject>,
    pub tags: Vec<crate::TrailTag>,
    pub include_redacted: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkTrailSelection {
    Forward {
        after: crate::FeedCursor,
    },
    Decisions {
        after: Option<crate::FeedCursor>,
        before: Option<crate::FeedCursor>,
        range_top: Option<crate::FeedCursor>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "page",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkTrailReadResult {
    Forward(crate::TrailPage),
    Decisions(crate::DecisionPage),
    CursorSuperseded { resume_from: crate::FeedCursor },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordListResult {
    pub records: Vec<crate::Record>,
    pub next_cursor: Option<String>,
}
/// The kind belongs to each named tool, not an unchecked caller-supplied table name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectReadKind {
    Workstream,
    Task,
    Subtask,
    WorkTrailRow,
    RoleAssignment,
    Move,
    Fact,
    NotifyIntent,
}
impl From<ProjectReadKind> for RecordKind {
    fn from(kind: ProjectReadKind) -> Self {
        match kind {
            ProjectReadKind::Workstream => Self::Workstream,
            ProjectReadKind::Task => Self::Task,
            ProjectReadKind::Subtask => Self::Subtask,
            ProjectReadKind::WorkTrailRow => Self::WorkTrailRow,
            ProjectReadKind::RoleAssignment => Self::RoleAssignment,
            ProjectReadKind::Move => Self::Move,
            ProjectReadKind::Fact => Self::Fact,
            ProjectReadKind::NotifyIntent => Self::NotifyIntent,
        }
    }
}
