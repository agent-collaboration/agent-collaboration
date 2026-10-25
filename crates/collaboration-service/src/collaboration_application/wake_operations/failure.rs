//! Wake failures retain the existing typed public guidance and uncertainty.
use super::*;
use crate::collaboration_application::{
    CollaborationRejection, CollaborationRejectionReason, PublishedRejection,
};
/// The identities a wake failure reports back, taken from the request that failed.
#[derive(Clone, Debug, Default)]
pub(crate) struct WakeFailureContext {
    pub(crate) operation_id: Option<OperationId>,
    pub(crate) wakeup_id: Option<WakeupId>,
}

impl WakeFailureContext {
    pub(super) fn operation(operation_id: OperationId) -> Self {
        Self {
            operation_id: Some(operation_id),
            wakeup_id: None,
        }
    }

    pub(super) fn wakeup(wakeup_id: WakeupId) -> Self {
        Self {
            operation_id: None,
            wakeup_id: Some(wakeup_id),
        }
    }
}

/// A wake failure with the stage, message and next action its reason implies.
pub(crate) fn wake_failure(
    context: WakeFailureContext,
    reason: WakeFailureReason,
    mutation: LocalMutationState,
) -> WakeFailure {
    let (stage,next_action,message)=match &reason {
        WakeFailureReason::InvalidField {constraint,..}=>(WakeFailureStage::Validation,WakeNextAction::CorrectRequest,constraint.clone()),
        WakeFailureReason::RemoteUnavailable=>(WakeFailureStage::Storage,if context.operation_id.is_some(){WakeNextAction::InspectOperation}else{WakeNextAction::RetryLater},"The owning Router is unavailable or its result was not confirmed. Replay the same operation identity for a mutation; do not create a new identity blindly.".into()),
        WakeFailureReason::DeadlineExpired=>(WakeFailureStage::Admission,WakeNextAction::InspectWakeup,"The remote wake request deadline elapsed. Inspect the owning wake or replay the original operation identity before creating another.".into()),
        WakeFailureReason::ResourceNotFound=>(WakeFailureStage::Inspection,WakeNextAction::VerifyResourceAddress,"Wake-up was not found in this service; verify the service and wakeupId.".into()),
        WakeFailureReason::OperationConflict=>(WakeFailureStage::Admission,WakeNextAction::InspectOperation,"Operation identity belongs to a different request; inspect it before submitting new work.".into()),
        WakeFailureReason::LifecycleConflict=>(WakeFailureStage::Validation,WakeNextAction::InspectWakeup,"This wake cannot perform that lifecycle transition. Inspect its state; cancelled or expired reminders are not implicitly recreated.".into()),
        WakeFailureReason::Overloaded=>(WakeFailureStage::Admission,WakeNextAction::RetryLater,"Request capacity exceeded; no wake mutation was dispatched.".into()),
        WakeFailureReason::Refused(reason)=>(WakeFailureStage::Admission,WakeNextAction::CorrectRequest,format!("Wake refused: {reason:?}")),
        _ if matches!(mutation,LocalMutationState::Unknown|LocalMutationState::Committed)=>(WakeFailureStage::Storage,WakeNextAction::InspectOperation,"Local mutation may exist; inspect or replay the same operation identity. Do not create a new identity blindly.".into()),
        _=>(WakeFailureStage::Storage,WakeNextAction::RetryLater,"Automation storage is unavailable or inconsistent; no new native submission was dispatched.".into()),
    };
    WakeFailure {
        reason,
        stage,
        message,
        operation_id: context.operation_id,
        wakeup_id: context.wakeup_id,
        effects: LocalMutationEvidence::Local { mutation },
        next_action,
    }
}

pub(crate) fn invalid_field(field: &str, constraint: &str) -> WakeFailureReason {
    WakeFailureReason::InvalidField {
        field: field.into(),
        constraint: constraint.into(),
    }
}

pub(super) fn storage_reason(error: StorageError) -> WakeFailureReason {
    match error {
        StorageError::WakeRefused(reason) => WakeFailureReason::Refused(reason),
        StorageError::WakeNotFound => WakeFailureReason::ResourceNotFound,
        StorageError::OperationConflict => WakeFailureReason::OperationConflict,
        StorageError::Database(_) => WakeFailureReason::AutomationUnavailable,
        StorageError::InvalidTiming(error) => WakeFailureReason::InvalidField {
            field: "timing".into(),
            constraint: error.to_string(),
        },
        StorageError::InvalidWake { field, reason } => WakeFailureReason::InvalidField {
            field: field.into(),
            constraint: reason.into(),
        },
        _ => WakeFailureReason::InvalidRecord,
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
