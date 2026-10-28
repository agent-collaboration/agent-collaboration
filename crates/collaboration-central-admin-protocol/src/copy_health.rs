//! Copy operations facts exposed by the owner HTTP API.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CopyStage {
    Starting,
    Seeding,
    Following,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum CopyHealthRead<Value> {
    Known(Value),
    Unreadable { detail: String },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyBookkeepingVersion {
    pub corpus: String,
    pub stored: Option<u64>,
    pub supported: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyMigrationFailureRecord {
    pub corpus: String,
    pub version: u64,
    pub reason: String,
    pub at: chrono::DateTime<chrono::Utc>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StagedPartHealth {
    pub transaction: fleet_sync_protocol::TransactionKey,
    pub index: u32,
    pub staged_at: chrono::DateTime<chrono::Utc>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiveCopyHealth {
    pub bookkeeping: CopyHealthRead<Vec<CopyBookkeepingVersion>>,
    pub migration_failures: CopyHealthRead<Vec<CopyMigrationFailureRecord>>,
    pub stage: CopyHealthRead<Option<CopyStage>>,
    pub copy: CopyHealthRead<Option<fleet_sync_protocol::CopyStatus>>,
    pub project_fingerprint: CopyHealthRead<String>,
    pub staged_parts: CopyHealthRead<Vec<StagedPartHealth>>,
    pub storage_bytes: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CopyHealth {
    Live(Box<LiveCopyHealth>),
    Retired {
        retired_at: chrono::DateTime<chrono::Utc>,
    },
}
