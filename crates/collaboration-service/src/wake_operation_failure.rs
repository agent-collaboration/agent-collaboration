//! A Wake's selected owner reports its own refusal without changing ownership on failure.
use crate::{
    CentralCatalogFailure,
    collaboration_rejection::{
        CollaborationRejection, CollaborationRejectionReason, PublishedRejection,
    },
};
use collaboration_protocol::{
    OperationId, WaitUnavailable, WakeFailure, WakeFailureReason, WakeNotFound, WakeupId,
};
use serde::Serialize;
use serde_json::json;

#[derive(Clone, Debug, Serialize, thiserror::Error)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WakeSelectionFailure {
    #[error("{message}")]
    HandoverPending {
        operation_id: OperationId,
        wakeup_id: WakeupId,
        message: String,
    },
    #[error("{message}")]
    LocalStoreUnavailable {
        operation_id: Option<OperationId>,
        wakeup_id: Option<WakeupId>,
        message: String,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum WakeOperationFailure {
    #[error("Wake wait rejected")]
    Wait(Box<WakeWaitFailure>),
    #[error("local Wake operation rejected: {0:?}")]
    Local(Box<WakeFailure>),
    #[error(transparent)]
    Catalog(Box<CentralCatalogFailure>),
    #[error(transparent)]
    Selection(WakeSelectionFailure),
}
impl From<WakeFailure> for WakeOperationFailure {
    fn from(error: WakeFailure) -> Self {
        Self::Local(Box::new(error))
    }
}
impl From<WakeWaitFailure> for WakeOperationFailure {
    fn from(error: WakeWaitFailure) -> Self {
        Self::Wait(Box::new(error))
    }
}
impl From<CentralCatalogFailure> for WakeOperationFailure {
    fn from(error: CentralCatalogFailure) -> Self {
        Self::Catalog(Box::new(error))
    }
}
impl From<WakeSelectionFailure> for WakeOperationFailure {
    fn from(error: WakeSelectionFailure) -> Self {
        Self::Selection(error)
    }
}
impl CollaborationRejection for WakeOperationFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self {
            Self::Wait(error) => error.rejection_reason(),
            Self::Local(error) => error.rejection_reason(),
            Self::Catalog(error) => error.rejection_reason(),
            Self::Selection(_) => None,
        }
    }
    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::Wait(error) => error.published_rejection(),
            Self::Local(error) => error.published_rejection(),
            Self::Catalog(error) => error.published_rejection(),
            Self::Selection(error) => PublishedRejection::typed(
                PublishedRejection::OPERATION_FAILED,
                error.to_string(),
                error,
            ),
        }
    }
}

/// Why a first-fire wait could not start.
#[derive(Clone, Debug)]
pub enum WakeWaitFailure {
    /// No wait capacity or storage; reconnect the wait without recreating the reminder.
    Unavailable(WaitUnavailable),
    NotFound(WakeNotFound),
    /// The wait's cursor or timeout is invalid.
    InvalidField {
        field: &'static str,
        constraint: &'static str,
    },
}

impl WakeWaitFailure {
    pub(crate) fn unavailable(wakeup_id: WakeupId) -> Self {
        Self::Unavailable(WaitUnavailable {
            kind: collaboration_protocol::WaitUnavailableKind::WaitUnavailable,
            stage: collaboration_protocol::WaitStage::WaitForFirstFire,
            message: "Wake wait unavailable; reconnect the wait without recreating or cancelling the reminder.".into(),
            wakeup_id,
            first_occurrence_id: None,
            effects: collaboration_protocol::WaitUnavailableEffects {
                first_fire: collaboration_protocol::UnknownFire::Unknown,
                wakeup_mutation: collaboration_protocol::NoMutation::None,
            },
            next_action: collaboration_protocol::WaitNextAction::ReconnectWait,
        })
    }

    pub(crate) fn not_found(wakeup_id: WakeupId) -> Self {
        Self::NotFound(WakeNotFound {
            kind: collaboration_protocol::WakeNotFoundKind::WakeNotFound,
            stage: collaboration_protocol::WaitStage::WaitForFirstFire,
            message: "Wake-up was not found in this service; verify the wake identity. Historical firing is unknown.".into(),
            wakeup_id,
            first_occurrence_id: None,
            effects: collaboration_protocol::UnknownFirstFire {
                first_fire: collaboration_protocol::UnknownFire::Unknown,
            },
            next_action: collaboration_protocol::VerifyWakeupAddress::VerifyWakeupAddress,
        })
    }
}

impl CollaborationRejection for WakeFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self.reason {
            WakeFailureReason::Overloaded => Some(CollaborationRejectionReason::Overloaded),
            WakeFailureReason::InvalidField { .. } => {
                Some(CollaborationRejectionReason::InvalidShape)
            }
            WakeFailureReason::OperationConflict => {
                Some(CollaborationRejectionReason::ConflictingRequest)
            }
            WakeFailureReason::AutomationUnavailable
            | WakeFailureReason::ResourceNotFound
            | WakeFailureReason::InvalidRecord
            | WakeFailureReason::Refused(_)
            | WakeFailureReason::LifecycleConflict
            | WakeFailureReason::RemoteUnavailable
            | WakeFailureReason::DeadlineExpired => None,
        }
    }

    fn published_rejection(&self) -> PublishedRejection {
        PublishedRejection::typed(
            PublishedRejection::OPERATION_FAILED,
            "Wake operation failed",
            self,
        )
    }
}

impl CollaborationRejection for WakeWaitFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self {
            Self::InvalidField { .. } => Some(CollaborationRejectionReason::InvalidShape),
            Self::Unavailable(_) | Self::NotFound(_) => None,
        }
    }

    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::Unavailable(data) => PublishedRejection::typed(
                PublishedRejection::OPERATION_FAILED,
                "Wake wait unavailable",
                data,
            ),
            Self::NotFound(data) => PublishedRejection::typed(
                PublishedRejection::OPERATION_FAILED,
                "Wake not found",
                data,
            ),
            Self::InvalidField { field, constraint } => PublishedRejection::typed(
                PublishedRejection::INVALID_PARAMS,
                *constraint,
                &json!({
                    "kind":"invalidField",
                    "stage":"waitForFirstFire",
                    "field":field,
                    "constraint":constraint,
                    "message":constraint,
                }),
            ),
        }
    }
}
