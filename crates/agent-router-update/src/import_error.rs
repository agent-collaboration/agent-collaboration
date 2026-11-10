use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum UpgradeError {
    #[error("legacy store is open in another process: {0}")]
    SourceInUse(PathBuf),
    #[error("cannot establish whether the legacy store is open: {0}")]
    OpenFileInspection(String),
    #[error("legacy {field} is invalid: {detail}")]
    InvalidLegacy { field: String, detail: String },
    #[error("legacy topic {} is invalid: {detail}", topic.as_str())]
    InvalidLegacyTopic {
        topic: message_board::ProjectId,
        detail: String,
    },
    #[error("legacy database read failed: {0}")]
    LegacyDatabase(#[from] rusqlite::Error),
    #[error("import file operation failed: {0}")]
    File(#[from] std::io::Error),
    #[error("import encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("legacy reader task failed: {0}")]
    ReaderTask(#[from] tokio::task::JoinError),
    #[error("project import failed: {0}")]
    Storage(#[from] message_board_storage::ProjectStorageError),
    #[error("project writer failed: {0}")]
    Writer(#[from] fleet_sync_writer::OpenError),
    #[error("project import write refused: {0}")]
    Write(#[from] fleet_sync_writer::WriteRefusal),
    #[error("fleet import is invalid: {0}")]
    Fleet(#[from] fleet_admission::FleetAdmissionError),
    #[error("project schema is unavailable: {0}")]
    Schema(#[from] board_fleet_sync::BoardProjectSchemaError),
    #[error("import snapshot is invalid: {0}")]
    Snapshot(String),
}

pub(crate) fn invalid(field: impl Into<String>, detail: impl ToString) -> UpgradeError {
    UpgradeError::InvalidLegacy {
        field: field.into(),
        detail: detail.to_string(),
    }
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(
    value: &str,
    field: &str,
) -> Result<T, UpgradeError> {
    serde_json::from_str(value).map_err(|error| invalid(field, error))
}
