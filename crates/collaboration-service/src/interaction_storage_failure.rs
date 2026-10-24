//! Translate the physical message owner's errors at the automation service boundary.
pub(crate) fn map_interaction_storage_error(
    failure: interaction_storage::StorageError,
) -> automation_storage::StorageError {
    use automation_storage::StorageError as AutomationError;
    use interaction_storage::StorageError as InteractionError;
    match failure {
        InteractionError::Database(source) => AutomationError::Database(source),
        InteractionError::InvalidRecord => AutomationError::InvalidRecord,
        InteractionError::PushAlreadyExists => AutomationError::PushAlreadyExists,
        InteractionError::PushContentConflict => AutomationError::PushContentConflict,
        InteractionError::PushNotFound => AutomationError::PushNotFound,
        InteractionError::PushStateConflict => AutomationError::PushStateConflict,
        InteractionError::PushNotPermitted => AutomationError::PushNotPermitted,
        InteractionError::StorageUnavailable
        | InteractionError::InvalidSchema
        | InteractionError::InstallationPeerConflict
        | InteractionError::InvalidStoredRecord => AutomationError::Database(
            sqlx::Error::Protocol("interaction storage is unavailable or invalid".to_owned()),
        ),
    }
}
