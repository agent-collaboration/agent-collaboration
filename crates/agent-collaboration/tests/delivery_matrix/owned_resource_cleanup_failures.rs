//! Cleanup refusals and unknown outcomes retain the selected owner's actual evidence.
use super::*;
pub(super) fn project_wake_failure(error: WakeClientError) -> CleanupOutcome {
    match error {
        WakeClientError::Rejected(failure) => {
            let failure = *failure;
            let kind = match failure.reason {
                WakeFailureReason::InvalidField { .. } => WakeCleanupFailureKind::InvalidField,
                WakeFailureReason::OperationConflict => WakeCleanupFailureKind::OperationConflict,
                WakeFailureReason::AutomationUnavailable => {
                    WakeCleanupFailureKind::AutomationUnavailable
                }
                WakeFailureReason::ResourceNotFound => WakeCleanupFailureKind::ResourceNotFound,
                WakeFailureReason::InvalidRecord => WakeCleanupFailureKind::InvalidRecord,
                WakeFailureReason::LifecycleConflict => WakeCleanupFailureKind::LifecycleConflict,
                WakeFailureReason::Overloaded => WakeCleanupFailureKind::Overloaded,
                WakeFailureReason::RemoteUnavailable => WakeCleanupFailureKind::RemoteUnavailable,
                WakeFailureReason::DeadlineExpired => WakeCleanupFailureKind::DeadlineExpired,
                WakeFailureReason::Refused(reason) => WakeCleanupFailureKind::Refused(reason),
            };
            let LocalMutationEvidence::Local { mutation } = failure.effects;
            CleanupOutcome::Rejected(ReportedCleanupFailure::Wake {
                kind,
                stage: failure.stage,
                operation_id: failure.operation_id,
                wakeup_id: failure.wakeup_id,
                mutation,
            })
        }
        WakeClientError::Selection(error) => {
            CleanupOutcome::Rejected(ReportedCleanupFailure::WakeSelection(error))
        }
        WakeClientError::Catalog(error) => project_catalog_failure(error),
        WakeClientError::Connection(error) => project_client_failure(error),
    }
}

pub(super) fn project_schedule_failure(error: ScheduleClientError) -> CleanupOutcome {
    match error {
        ScheduleClientError::Rejected(failure) => {
            let failure = *failure;
            let effect = match failure.effects {
                ScheduleEffects::Local { mutation } => ReportedCleanupEffect::Local(mutation),
                ScheduleEffects::Route { .. } => ReportedCleanupEffect::Route,
            };
            CleanupOutcome::Rejected(ReportedCleanupFailure::Schedule {
                kind: failure.kind,
                stage: failure.stage,
                operation_id: failure.operation_id,
                schedule_id: failure.schedule_id,
                current_change_id: failure.current_change_id,
                effect,
            })
        }
        ScheduleClientError::Selection(error) => {
            CleanupOutcome::Rejected(ReportedCleanupFailure::ScheduleSelection(error))
        }
        ScheduleClientError::Catalog(error) => project_catalog_failure(error),
        ScheduleClientError::ReadFailure(error) => {
            CleanupOutcome::Rejected(ReportedCleanupFailure::ReadFault(error))
        }
        ScheduleClientError::Connection(error) => project_client_failure(error),
    }
}

pub(super) fn project_board_failure(error: BoardClientError) -> CleanupOutcome {
    match error {
        BoardClientError::Rejected(failure) => {
            CleanupOutcome::Rejected(ReportedCleanupFailure::Board {
                kind: failure.kind,
                stage: failure.stage,
            })
        }
        BoardClientError::OutcomeUnknown { .. } => {
            CleanupOutcome::Unknown(CleanupUncertainty::BoardOutcomeUnknown)
        }
        BoardClientError::WaitOutcomeUnknown { .. } => {
            CleanupOutcome::Unknown(CleanupUncertainty::UnexpectedWaitOutcomeUnknown)
        }
        BoardClientError::Connection(error) => project_client_failure(error),
    }
}

pub(super) fn project_client_failure(error: ClientError) -> CleanupOutcome {
    let kind = match error {
        ClientError::InvalidRequest(_) => ClientFailureKind::InvalidRequest,
        ClientError::UnsupportedCapability(_) => ClientFailureKind::UnsupportedCapability,
        ClientError::Discovery { .. } => ClientFailureKind::Discovery,
        ClientError::Transport(error) => ClientFailureKind::Transport(error.kind()),
        ClientError::Protocol(_) => ClientFailureKind::Protocol,
        ClientError::Timeout => ClientFailureKind::Timeout,
        ClientError::Rejected { code, .. } => ClientFailureKind::RpcRejected { code },
        ClientError::Overloaded { .. } => ClientFailureKind::Overloaded,
    };
    CleanupOutcome::Unknown(CleanupUncertainty::Client(kind))
}

fn project_catalog_failure(error: collaboration_client::CentralCatalogFailure) -> CleanupOutcome {
    match error {
        error @ collaboration_client::CentralCatalogFailure::Refused { .. } => {
            CleanupOutcome::Rejected(ReportedCleanupFailure::Catalog(Box::new(error)))
        }
        error => CleanupOutcome::Unknown(CleanupUncertainty::Catalog(Box::new(error))),
    }
}

impl std::fmt::Debug for ReportedCleanupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Catalog(value) => f.debug_tuple("Catalog").field(value).finish(),
            Self::CatalogFault(value) => f.debug_tuple("CatalogFault").field(value).finish(),
            Self::ReadFault(value) => f.debug_tuple("ReadFault").field(value).finish(),
            Self::WakeSelection(value) => f.debug_tuple("WakeSelection").field(value).finish(),
            Self::ScheduleSelection(value) => {
                f.debug_tuple("ScheduleSelection").field(value).finish()
            }
            Self::Wake {
                kind,
                stage,
                operation_id,
                wakeup_id,
                mutation,
            } => f
                .debug_struct("Wake")
                .field("kind", kind)
                .field("stage", stage)
                .field("operation_id", operation_id)
                .field("wakeup_id", wakeup_id)
                .field("mutation", mutation)
                .finish(),
            Self::Schedule {
                kind,
                stage,
                operation_id,
                schedule_id,
                current_change_id,
                effect,
            } => f
                .debug_struct("Schedule")
                .field("kind", kind)
                .field("stage", stage)
                .field("operation_id", operation_id)
                .field("schedule_id", schedule_id)
                .field("current_change_id", current_change_id)
                .field("effect", effect)
                .finish(),
            Self::Board { kind, stage } => f
                .debug_struct("Board")
                .field("kind", kind)
                .field("stage", stage)
                .finish(),
        }
    }
}
impl std::fmt::Debug for CleanupUncertainty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Catalog(value) => f.debug_tuple("Catalog").field(value).finish(),
            Self::Client(value) => f.debug_tuple("Client").field(value).finish(),
            Self::BoardOutcomeUnknown => f.write_str("BoardOutcomeUnknown"),
            Self::UnexpectedWaitOutcomeUnknown => f.write_str("UnexpectedWaitOutcomeUnknown"),
            Self::ResponseIdentityMismatch => f.write_str("ResponseIdentityMismatch"),
            Self::ReadbackUnavailable => f.write_str("ReadbackUnavailable"),
        }
    }
}
