//! Wake failures retain the existing typed public guidance and uncertainty.
use super::{
    LocalMutationEvidence, LocalMutationState, OperationId, WakeFailure, WakeFailureReason,
    WakeFailureStage, WakeNextAction, WakeupId,
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
