#[derive(Debug, thiserror::Error)]
pub enum FleetStorageError {
    #[error("fleet storage is unavailable: {0}")]
    Database(#[from] sqlx::Error),
    #[error("fleet admission cannot decide: {0}")]
    Admission(#[from] fleet_admission::FleetAdmissionError),
    #[error("fleet stored record, feed or migration is invalid")]
    InvalidRecord,
    #[error("fleet stored record is invalid")]
    InvalidRecordAt { record: message_board::RecordRef },
    #[error("fleet stored identity is invalid in {field}")]
    InvalidRecordKey { field: &'static str },
    #[error("fleet stored data cannot be encoded: {0}")]
    Encoding(#[from] serde_json::Error),
}
