//! Errors cross the storage boundary without depending on its physical owner.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("interaction database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("interaction storage is unavailable")]
    StorageUnavailable,
    #[error("interaction schema is not owned or supported")]
    InvalidSchema,
    #[error("interaction storage contains an invalid stored record")]
    InvalidStoredRecord,
    #[error("interaction record is invalid or inconsistent")]
    InvalidRecord,
}
