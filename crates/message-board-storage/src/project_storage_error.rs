use message_board::{RecordRef, SchemaRefusal};

#[derive(Debug, thiserror::Error)]
pub enum ProjectStorageError {
    #[error("project storage is unavailable: {0}")]
    Database(#[from] sqlx::Error),
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
    #[error("project move is pending: {reason}")]
    MoveAsidePending { reason: String },
    #[error("project file operation failed: {0}")]
    File(#[from] std::io::Error),
    #[error("stored data cannot be encoded: {0}")]
    Encoding(#[from] serde_json::Error),
}
