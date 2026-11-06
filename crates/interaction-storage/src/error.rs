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
    #[error("push record id already exists")]
    PushAlreadyExists,
    #[error("push identity already belongs to different content")]
    PushContentConflict,
    #[error("push record was not found in this interaction database")]
    PushNotFound,
    #[error("push record delivery state does not permit this transition")]
    PushStateConflict,
    #[error("push record is not permitted for this caller")]
    PushNotPermitted,
    #[error("installation identities conflict with the immutable local directory")]
    InstallationPeerConflict,
}
