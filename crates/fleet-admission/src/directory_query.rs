use chrono::{DateTime, Utc};
use collaboration_replication_protocol::{DirectoryLease, InstallationId, ReplicationPosition};
use message_board::{BoardId, BoardState, ProjectId, ResourceName, WorkspaceId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryQuery {
    pub text: Option<String>,
    pub workspace: Option<WorkspaceId>,
    pub board: Option<BoardId>,
    pub status: Option<String>,
    pub holder: Option<InstallationId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DirectorySide<TMetadata> {
    Known(TMetadata),
    NotYetReported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyMetadataAsOf {
    pub position: ReplicationPosition,
    pub acked_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyMetadata {
    pub status: String,
    pub latest_record_at: DateTime<Utc>,
    pub as_of: CopyMetadataAsOf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseMetadata {
    pub lease: DirectoryLease,
    pub as_of: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryEntry {
    pub project: ProjectId,
    pub name: ResourceName,
    pub workspace: WorkspaceId,
    pub board: BoardId,
    pub board_archive: BoardState,
    pub copy_side: DirectorySide<CopyMetadata>,
    pub lease_side: DirectorySide<LeaseMetadata>,
}
