use chrono::{DateTime, Utc};
use fleet_sync_protocol::{Epoch, ReplicationPosition, SyncTime};
use message_board::LeaseVersion;
use message_board::{
    BoardId, BoardState, ProjectId, RecordTime, ResourceName, ServiceId, StatusName, WorkspaceId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryQuery {
    pub text: Option<String>,
    pub workspace: Option<WorkspaceId>,
    pub board: Option<BoardId>,
    pub status: Option<String>,
    pub holder: Option<ServiceId>,
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
    pub acked_at: SyncTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyMetadata {
    pub status: Option<StatusName>,
    pub latest_record_at: RecordTime,
    pub as_of: CopyMetadataAsOf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseMetadata {
    pub lease: DirectoryLease,
    pub lease_version: LeaseVersion,
    pub as_of: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DirectoryLease {
    Held {
        epoch: Epoch,
        holder: ServiceId,
        state: DirectoryLeaseState,
        expires_at: DateTime<Utc>,
    },
    Unheld,
    Retired {
        retired_at: DateTime<Utc>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectoryLeaseState {
    Held,
    Expired,
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

impl DirectoryQuery {
    pub fn matches(&self, entry: &DirectoryEntry) -> bool {
        if self.text.as_ref().is_some_and(|text| {
            !entry
                .name
                .as_str()
                .to_lowercase()
                .contains(&text.to_lowercase())
        }) || self
            .workspace
            .as_ref()
            .is_some_and(|workspace| workspace != &entry.workspace)
            || self
                .board
                .as_ref()
                .is_some_and(|board| board != &entry.board)
        {
            return false;
        }
        if let Some(status) = &self.status {
            match &entry.copy_side {
                DirectorySide::Known(metadata)
                    if metadata
                        .status
                        .as_ref()
                        .map(message_board::StatusName::as_str)
                        == Some(status.as_str()) => {}
                DirectorySide::Known(_) | DirectorySide::NotYetReported => return false,
            }
        }
        if let Some(holder) = &self.holder {
            match &entry.lease_side {
                DirectorySide::Known(LeaseMetadata {
                    lease: DirectoryLease::Held { holder: actual, .. },
                    ..
                }) if actual == holder => {}
                DirectorySide::Known(_) | DirectorySide::NotYetReported => return false,
            }
        }
        true
    }
}
