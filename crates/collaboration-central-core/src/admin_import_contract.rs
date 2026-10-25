//! The admin workflow records completion separately from a library reservation.

use board_sync_protocol::BoardAnnotation;
use fleet_sync_protocol::{
    DatabaseId, ImportDigest, PageDigest, ReplicationPosition, RequestId, SeedId, SnapshotManifest,
    SnapshotPage,
};
use serde::{Deserialize, Serialize};

/// Physical integrity belongs to the manifest; semantic identity to the digest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdminImportHeader {
    pub digest: ImportDigest,
    pub provenance: fleet_admission::ImportProvenance,
    pub declared_owner: message_board::HumanId,
    pub manifest: SnapshotManifest,
    pub pages: u32,
    pub summary: BoardAnnotation,
}

impl AdminImportHeader {
    pub fn database(&self) -> DatabaseId {
        self.manifest.database
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AdminImportPhase {
    Reserved,
    Published { position: ReplicationPosition },
    Complete { position: ReplicationPosition },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdminImportRecord {
    pub import_id: RequestId,
    pub header: AdminImportHeader,
    pub phase: AdminImportPhase,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AdminProjectImportRequest {
    Begin {
        header: AdminImportHeader,
    },
    Page {
        digest: ImportDigest,
        import_id: RequestId,
        page: u32,
        contents: SnapshotPage,
        page_digest: PageDigest,
    },
    Complete {
        digest: ImportDigest,
        import_id: RequestId,
    },
    Status {
        digest: ImportDigest,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AdminProjectImportAnswer {
    Ready {
        import_id: RequestId,
        seed: SeedId,
    },
    Staged {
        import_id: RequestId,
        page: u32,
    },
    Progress(AdminImportRecord),
    Imported {
        head: ReplicationPosition,
    },
    AlreadyImported,
    ImportConflict {
        existing: Option<ImportDigest>,
        offered: ImportDigest,
    },
    Retired,
    Pending,
    CopyUnavailable {
        reason: fleet_sync_protocol::CopyUnavailableReason,
    },
    OutcomeUnknown {
        reason: fleet_sync_protocol::OutcomeUnknownReason,
    },
    Refused(crate::admin_import_store::AdminImportRefusal),
}

impl AdminImportRecord {
    pub(crate) fn publish(&mut self, position: ReplicationPosition) -> bool {
        if position != self.header.manifest.position {
            return false;
        }
        match &self.phase {
            AdminImportPhase::Reserved => {
                self.phase = AdminImportPhase::Published { position };
                true
            }
            AdminImportPhase::Published { position: saved }
            | AdminImportPhase::Complete { position: saved } => *saved == position,
        }
    }

    pub(crate) fn complete(&mut self) -> bool {
        match self.phase {
            AdminImportPhase::Reserved => false,
            AdminImportPhase::Published { position } => {
                self.phase = AdminImportPhase::Complete { position };
                true
            }
            AdminImportPhase::Complete { .. } => true,
        }
    }
}
