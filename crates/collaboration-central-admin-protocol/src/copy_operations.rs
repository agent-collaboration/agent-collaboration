//! Bounded copy enumeration and migration results.
use message_board::ProjectId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyDirectoryPage {
    pub projects: Vec<fleet_admission::DirectoryEntry>,
    pub next_after: Option<ProjectId>,
    pub as_of: chrono::DateTime<chrono::Utc>,
}
/// A missing version stays absent; it is never encoded as version zero.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CopyMigrationResult {
    Migrated {
        corpus: String,
        from: Option<u64>,
        to: u64,
    },
    Current {
        corpus: String,
        version: u64,
    },
    Failed {
        corpus: String,
        version: Option<u64>,
        reason: String,
    },
    Retired,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyMigrationItem {
    pub project: ProjectId,
    pub result: CopyMigrationResult,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyMigrationPage {
    pub results: Vec<CopyMigrationItem>,
    pub next_after: Option<ProjectId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyPageQuery {
    pub after: Option<ProjectId>,
    pub limit: Option<u16>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CopyRepairAnswer {
    Repaired,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum OwnerOperationFailure {
    InvalidPageLimit,
    ObjectUnavailable,
    Retired,
    CentralCopyFull,
    CopyUnavailable {
        reason: fleet_sync_protocol::CopyUnavailableReason,
    },
}
