//! Keep shared failures and retryable project failures distinct from quarantine.
use crate::ProjectRegistryError;
use message_board::*;
use message_board_storage::ProjectStorageError;

pub(in crate::service_identity::subscription_delivery) enum WindowStoreError {
    Project {
        project: ProjectId,
        error: BoardError,
    },
    RetryProject {
        project: ProjectId,
        error: BoardError,
        diagnostic: Box<str>,
    },
    Shared {
        error: BoardError,
        diagnostic: Option<Box<str>>,
    },
}
impl From<BoardError> for WindowStoreError {
    fn from(error: BoardError) -> Self {
        Self::Shared {
            error,
            diagnostic: None,
        }
    }
}
impl From<WindowStoreError> for BoardError {
    fn from(error: WindowStoreError) -> Self {
        match error {
            WindowStoreError::RetryProject {
                error, diagnostic, ..
            } => {
                tracing::warn!(error = %diagnostic, "subscription window storage unavailable");
                error
            }
            WindowStoreError::Shared { error, diagnostic } => {
                if let Some(diagnostic) = diagnostic {
                    tracing::warn!(error = %diagnostic, "subscription delivery progress storage unavailable");
                }
                error
            }
            WindowStoreError::Project { error, .. } => error,
        }
    }
}
pub(super) fn project_storage_failure(
    project: &ProjectId,
    error: ProjectStorageError,
) -> WindowStoreError {
    match error {
        definitive @ (ProjectStorageError::InvalidRecord { .. }
        | ProjectStorageError::InvalidFeed
        | ProjectStorageError::InvalidSchema
        | ProjectStorageError::SchemaNotReady(_)) => WindowStoreError::Project {
            project: project.clone(),
            error: invalid_membership(definitive),
        },
        transient => WindowStoreError::RetryProject {
            project: project.clone(),
            error: BoardError::board_unavailable(),
            diagnostic: transient.to_string().into_boxed_str(),
        },
    }
}
pub(super) fn project_registry_failure(
    project: &ProjectId,
    error: ProjectRegistryError,
) -> WindowStoreError {
    match error {
        ProjectRegistryError::Storage(error) => project_storage_failure(project, error),
        // This project was already quarantined by a definitive native failure.
        definitive @ ProjectRegistryError::Unavailable { .. } => WindowStoreError::Project {
            project: project.clone(),
            error: unavailable(definitive),
        },
        transient => WindowStoreError::RetryProject {
            project: project.clone(),
            error: BoardError::board_unavailable(),
            diagnostic: transient.to_string().into_boxed_str(),
        },
    }
}
pub(super) fn shared_storage_failure(error: impl std::fmt::Display) -> WindowStoreError {
    WindowStoreError::Shared {
        error: BoardError::board_unavailable(),
        diagnostic: Some(error.to_string().into_boxed_str()),
    }
}
pub(super) fn unavailable<TError: std::fmt::Display>(error: TError) -> BoardError {
    tracing::warn!(%error,"subscription window storage unavailable");
    BoardError::board_unavailable()
}
pub(super) fn invalid_window() -> BoardError {
    BoardError {
        kind: BoardFailureKind::InvalidRecord,
        stage: BoardFailureStage::Storage,
        message: "Stored subscription delivery progress is invalid.".to_owned(),
        next_action: BoardNextAction::InspectResource,
        details: BoardErrorDetails::None,
    }
}
pub(super) fn invalid_membership(error: ProjectStorageError) -> BoardError {
    let field = match &error {
        ProjectStorageError::InvalidRecord { field, .. } => *field,
        _ => return unavailable(error),
    };
    BoardError {
        kind: BoardFailureKind::InvalidRecord,
        stage: BoardFailureStage::Inspection,
        message: error.to_string(),
        next_action: BoardNextAction::InspectResource,
        details: BoardErrorDetails::FieldConstraint {
            field: field.to_owned(),
            requirement: "must contain a valid stored domain value".to_owned(),
        },
    }
}
