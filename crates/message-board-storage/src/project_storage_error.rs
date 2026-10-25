use message_board::{RecordRef, SchemaRefusal};

#[derive(Debug, thiserror::Error)]
pub enum ProjectStorageError {
    #[error("project storage is unavailable: {0}")]
    Database(sqlx::Error),
    #[error("project write was refused: {0}")]
    WriteRefused(#[from] fleet_sync_writer::WriteRefusal),
    #[error("project writer is unavailable: {0}")]
    Writer(#[from] fleet_sync_writer::OpenError),
    #[error("import row key for {kind:?} is not a canonical UUIDv7")]
    InvalidImportRowKey { kind: crate::ImportRowKind },
    #[error("stored actor is invalid in {field}")]
    InvalidActor { field: &'static str },
    #[error("project schema is not ready: {0:?}")]
    SchemaNotReady(SchemaRefusal),
    #[error("project schema or migration history is invalid")]
    InvalidSchema,
    #[error("stored record {record:?} is invalid in {field}")]
    InvalidRecord {
        record: RecordRef,
        field: &'static str,
    },
    #[error("stored feed or checkpoint is invalid")]
    InvalidFeed,
    #[error("the published snapshot has no matching local capture baseline yet")]
    SnapshotBaselinePending,
    #[error("feed cursor is superseded; resume at {resume_from:?}")]
    CursorSuperseded {
        resume_from: message_board::FeedCursor,
    },
    #[error("feed cursor does not name an accepted position or a superseded line")]
    InvalidCursor,
    #[error("retained project reader is unavailable")]
    RetainedFileUnavailable,
    #[error("project move is pending: {reason}")]
    MoveAsidePending { reason: String },
    #[error("project file operation failed: {0}")]
    File(#[from] std::io::Error),
    #[error("stored data cannot be encoded: {0}")]
    Encoding(#[from] serde_json::Error),
}

impl From<sqlx::Error> for ProjectStorageError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Configuration(source) = &error
            && let Some(refusal) = source.downcast_ref::<fleet_sync_writer::WriteRefusal>()
        {
            return Self::WriteRefused(refusal.clone());
        }
        Self::Database(error)
    }
}
