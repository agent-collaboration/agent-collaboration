use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BlockedSubject {
    Transfer { transfer: crate::TransferId },
    Project { project: crate::ProjectId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BookkeepingKind {
    Copy,
    Registry,
    Fleet,
    Connection,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CopyUnavailableReason {
    Uninitialized,
    StorageFull,
    Damaged { detail: String },
    MigrationPending { kind: BookkeepingKind, version: u64 },
}
