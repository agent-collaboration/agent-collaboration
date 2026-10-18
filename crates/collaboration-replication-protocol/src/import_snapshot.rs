use crate::{
    DigestError, PieceDigest, ProjectId, ProjectSummary, ReplicationPosition, SchemaIdentity,
    SnapshotPage, SqlValue,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ImportDigest(PieceDigest);
impl ImportDigest {
    pub fn new(value: String) -> Result<Self, DigestError> {
        PieceDigest::try_from(value).map(Self)
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSnapshotHeader {
    pub project: ProjectId,
    pub head: ReplicationPosition,
    pub schema: SchemaIdentity,
    pub schema_steps: Vec<String>,
}
impl From<&SnapshotPage> for ImportSnapshotHeader {
    fn from(page: &SnapshotPage) -> Self {
        Self {
            project: page.project.clone(),
            head: page.position,
            schema: page.schema.clone(),
            schema_steps: page.schema_steps.clone(),
        }
    }
}
/// Feed rows in snapshot order: table name, then declared primary key.
/// Each canonical JSON item has an eight-byte big-endian byte-length prefix.
/// Transport pages, their cursors and freshness are deliberately absent.
pub struct ImportDigestBuilder(Sha256);
impl ImportDigestBuilder {
    pub fn new(header: &ImportSnapshotHeader) -> Result<Self, DigestError> {
        let mut builder = Self(Sha256::new());
        builder.item(&(
            &header.project,
            header.head,
            &header.schema,
            &header.schema_steps,
        ))?;
        Ok(builder)
    }
    fn item<TContent: Serialize>(&mut self, content: &TContent) -> Result<(), DigestError> {
        let bytes =
            serde_json::to_vec(content).map_err(|e| DigestError::Encoding(e.to_string()))?;
        let length =
            u64::try_from(bytes.len()).map_err(|e| DigestError::Encoding(e.to_string()))?;
        self.0.update(length.to_be_bytes());
        self.0.update(bytes);
        Ok(())
    }
    pub fn append_row(&mut self, table: &str, row: &[SqlValue]) -> Result<(), DigestError> {
        self.item(&(table, row))
    }
    pub fn finish(mut self, summary: &ProjectSummary) -> Result<ImportDigest, DigestError> {
        self.item(summary)?;
        let hash = self.0.finalize();
        let mut hexadecimal = String::with_capacity(64);
        use std::fmt::Write;
        for byte in hash {
            write!(hexadecimal, "{byte:02x}").map_err(|e| DigestError::Encoding(e.to_string()))?;
        }
        ImportDigest::new(hexadecimal)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectImportPage {
    pub digest: ImportDigest,
    pub index: u64,
    pub page: SnapshotPage,
    pub final_summary: Option<ProjectSummary>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ExistingImport {
    Imported(ImportDigest),
    NotImported,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ProjectImportRefusal {
    UnexpectedIndex {
        expected: u64,
    },
    InvalidPage {
        field: String,
    },
    DigestMismatch {
        computed: ImportDigest,
        offered: ImportDigest,
    },
    SchemaMismatch,
    SchemaUnsupported {
        form: crate::SchemaStatementForm,
    },
    CentralCopyFull,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ProjectImportAnswer {
    Retired,
    Staged {
        digest: ImportDigest,
        index: u64,
    },
    Imported {
        head: ReplicationPosition,
    },
    AlreadyImported,
    ImportConflict {
        existing: ExistingImport,
        offered: ImportDigest,
    },
    Refused(ProjectImportRefusal),
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
        holder: crate::InstallationId,
        state: crate::LeaseState,
        expires_at: chrono::DateTime<chrono::Utc>,
    },
    Unheld,
}
