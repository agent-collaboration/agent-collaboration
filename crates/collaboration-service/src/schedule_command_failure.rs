//! Schedule refusal models and their public evidence have no application dependency.
use crate::collaboration_rejection::{
    CollaborationRejection, CollaborationRejectionReason, PublishedRejection,
};
use crate::{CentralCatalogFailure, ScheduleSelectionFailure};
use automation_storage::StorageError;
use collaboration_protocol::{
    ChangeId, LocalMutationState, OperationId, ScheduleEffects, ScheduleFailure,
    ScheduleFailureKind, ScheduleFailureStage, ScheduleId, ScheduleNextAction,
};

#[derive(Debug, thiserror::Error)]
pub enum ScheduleCommandFailure {
    #[error(transparent)]
    Local(Box<ScheduleOperationFailure>),
    #[error(transparent)]
    Catalog(Box<CentralCatalogFailure>),
    #[error(transparent)]
    Selection(Box<ScheduleSelectionFailure>),
}
impl From<ScheduleOperationFailure> for ScheduleCommandFailure {
    fn from(value: ScheduleOperationFailure) -> Self {
        Self::Local(Box::new(value))
    }
}
impl From<CentralCatalogFailure> for ScheduleCommandFailure {
    fn from(value: CentralCatalogFailure) -> Self {
        Self::Catalog(Box::new(value))
    }
}
impl From<ScheduleSelectionFailure> for ScheduleCommandFailure {
    fn from(value: ScheduleSelectionFailure) -> Self {
        Self::Selection(Box::new(value))
    }
}
impl CollaborationRejection for ScheduleCommandFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self {
            Self::Local(value) => value.rejection_reason(),
            Self::Catalog(value) => value.rejection_reason(),
            Self::Selection(_) => None,
        }
    }
    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::Local(value) => value.published_rejection(),
            Self::Catalog(value) => value.published_rejection(),
            Self::Selection(value) => PublishedRejection::typed(
                PublishedRejection::OPERATION_FAILED,
                value.to_string(),
                &**value,
            ),
        }
    }
}

/// The identities a schedule failure reports back, taken from the request that failed.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScheduleFailureContext {
    pub(crate) operation_id: Option<OperationId>,
    pub(crate) schedule_id: Option<ScheduleId>,
}

impl ScheduleFailureContext {
    pub(crate) fn new(operation_id: Option<OperationId>, schedule_id: Option<ScheduleId>) -> Self {
        Self {
            operation_id,
            schedule_id,
        }
    }
}

/// A schedule failure with the kind, stage, field and next action its storage error implies.
pub(crate) fn schedule_failure(
    context: ScheduleFailureContext,
    error: StorageError,
    mutation: LocalMutationState,
    current_change_id: Option<ChangeId>,
) -> ScheduleFailure {
    let (kind, stage, next_action, field, constraint) = match &error {
        StorageError::ScheduleImportExists => (
            ScheduleFailureKind::InvalidField, ScheduleFailureStage::Admission,
            ScheduleNextAction::CorrectRequest, Some("overwrite".into()),
            Some("An existing schedule UUID requires explicit --overwrite, even for identical content.".into()),
        ),
        StorageError::InstructionImportConflict { .. } => (
            ScheduleFailureKind::InstructionConflict, ScheduleFailureStage::Admission,
            ScheduleNextAction::CorrectRequest, Some("packageUtf8".into()),
            Some("Edit the shared instructions explicitly or import a deliberately new instruction identity; --overwrite applies only to the schedule.".into()),
        ),
        StorageError::InvalidPackage(error) => (
            ScheduleFailureKind::InvalidField, ScheduleFailureStage::Validation,
            ScheduleNextAction::CorrectRequest, Some("packageUtf8".into()), Some(error.to_string()),
        ),
        StorageError::InvalidSchedule { field, reason } => (
            ScheduleFailureKind::InvalidField,
            ScheduleFailureStage::Validation,
            ScheduleNextAction::CorrectRequest,
            Some((*field).to_owned()),
            Some((*reason).to_owned()),
        ),
        StorageError::InvalidTiming(error) => (
            ScheduleFailureKind::InvalidField,
            ScheduleFailureStage::Validation,
            ScheduleNextAction::CorrectRequest,
            Some("timing".into()),
            Some(error.to_string()),
        ),
        StorageError::ActivationUnavailable => (
            ScheduleFailureKind::UnsupportedCapability,
            ScheduleFailureStage::Admission,
            ScheduleNextAction::InspectEndpointCapabilities,
            None,
            None,
        ),
        StorageError::ScheduleChangeConflict => (
            ScheduleFailureKind::ChangeConflict,
            ScheduleFailureStage::Admission,
            ScheduleNextAction::InspectSchedule,
            None,
            None,
        ),
        StorageError::OperationConflict => (
            ScheduleFailureKind::OperationConflict,
            ScheduleFailureStage::Admission,
            ScheduleNextAction::InspectOperation,
            None,
            None,
        ),
        StorageError::ScheduleNotFound | StorageError::InstructionNotFound => (
            ScheduleFailureKind::ResourceNotFound,
            ScheduleFailureStage::Inspection,
            ScheduleNextAction::CorrectRequest,
            None,
            None,
        ),
        StorageError::Database(_) => (
            ScheduleFailureKind::AutomationUnavailable,
            ScheduleFailureStage::Storage,
            ScheduleNextAction::InspectOperation,
            None,
            None,
        ),
        _ => (
            ScheduleFailureKind::InvalidRecord,
            ScheduleFailureStage::Storage,
            ScheduleNextAction::InspectSchedule,
            None,
            None,
        ),
    };
    let message = if matches!(
        mutation,
        LocalMutationState::Unknown | LocalMutationState::Committed
    ) {
        "Schedule mutation may exist; inspect or replay the same operation ID before any new request.".into()
    } else {
        error.to_string()
    };
    ScheduleFailure {
        kind,
        stage,
        message,
        operation_id: context.operation_id,
        schedule_id: context.schedule_id,
        current_change_id,
        field,
        constraint,
        details: match error {
            StorageError::InstructionImportConflict {
                instruction_id,
                schedule_ids,
            } => collaboration_protocol::ScheduleFailureDetails::InstructionConflict {
                instruction_id,
                schedule_ids,
            },
            _ => collaboration_protocol::ScheduleFailureDetails::None,
        },
        effects: ScheduleEffects::Local { mutation },
        next_action,
    }
}

/// Why a schedule operation failed; every variant carries the typed `ScheduleFailure`.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ScheduleOperationFailure {
    /// Local administration or a package transfer was refused or could not complete.
    #[error("Schedule operation failed")]
    Operation(ScheduleFailure),
    /// The package, or the request that would import it, exceeds the response limit.
    #[error("Schedule package too large")]
    PackageTooLarge(ScheduleFailure),
    /// Preparation was refused or its route failed.
    #[error("Schedule preparation failed")]
    Preparation(ScheduleFailure),
    /// A retried preparation returns the failure the original attempt recorded.
    #[error("Retained preparation outcome")]
    RetainedPreparation(ScheduleFailure),
}

impl ScheduleOperationFailure {
    /// The typed failure this variant carries.
    #[must_use]
    pub const fn failure(&self) -> &ScheduleFailure {
        match self {
            Self::Operation(failure)
            | Self::PackageTooLarge(failure)
            | Self::Preparation(failure)
            | Self::RetainedPreparation(failure) => failure,
        }
    }
}

impl CollaborationRejection for ScheduleOperationFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self {
            Self::PackageTooLarge(_) => None,
            Self::Operation(failure)
            | Self::Preparation(failure)
            | Self::RetainedPreparation(failure) => failure.rejection_reason(),
        }
    }

    fn published_rejection(&self) -> PublishedRejection {
        PublishedRejection::typed(
            PublishedRejection::OPERATION_FAILED,
            self.to_string(),
            self.failure(),
        )
    }
}
