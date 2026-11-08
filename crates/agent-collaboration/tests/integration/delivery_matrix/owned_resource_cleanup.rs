//! One-pass cleanup mutations followed by reads of the existing M2 state owners.
#[path = "owned_resource_cleanup_readback.rs"]
pub(super) mod readback;
use super::owned_resources::{
    CleanupIntent, MatrixCleanupPlan, MatrixCreationHistory, UnresolvedCreation,
};
use collaboration_client::{
    BoardClientError, ClientError, CollaborationClient, ScheduleClientError, WakeClientError,
    board::{
        BoardFailureKind, BoardFailureStage, Identity, SubscriptionScope, ThreadId,
        ThreadUnwatchRequest,
    },
    protocol::{
        ChangeId, DeliveryDisposition, DeliveryId, LocalMutationEvidence, LocalMutationState,
        ObservationTimestamp, OperationId, RunId, ScheduleEffects, ScheduleEnableRequest,
        ScheduleFailureKind, ScheduleFailureStage, ScheduleId, ThreadSubscriptionState,
        ThreadSubscriptionsRequest, ThreadUnsubscribeRequest, WakeFailureReason, WakeFailureStage,
        WakeMutationRequest, WakeMutationResult, WakeState, WakeupId,
    },
};

/// C retains this same client and owner outside its caught scenario, before first mutation.
pub(super) struct OwnedMatrixScope {
    pub(super) client: CollaborationClient,
    pub(super) resources: super::owned_resources::OwnedMatrixResources,
}
impl OwnedMatrixScope {
    pub(super) fn new(client: CollaborationClient) -> Self {
        Self {
            client,
            resources: super::owned_resources::OwnedMatrixResources::default(),
        }
    }

    pub(super) async fn cleanup(self) -> super::ProofResult<()> {
        use std::io::Write as _;
        let report =
            cleanup_owned_resources(&self.client, self.resources.into_cleanup_plan()).await;
        // All actual attempts/history survive the scenario error/panic reduction as metadata.
        let reporting_error =
            writeln!(std::io::stderr(), "matrix resource cleanup: {report:?}").err();
        let returned_local_retirement = report.unresolved.is_empty()
            && report.attempts.iter().all(|attempt| {
                matches!(
                    &attempt.outcome,
                    CleanupOutcome::Returned(CleanupObservation::Wake {
                        state: WakeState::Cancelled | WakeState::Expired | WakeState::Finished,
                        next_due_at: None,
                        ..
                    }) | CleanupOutcome::Returned(CleanupObservation::Schedule {
                        enabled: false,
                        next_due_at: None,
                        ..
                    }) | CleanupOutcome::Returned(CleanupObservation::Subscription {
                        delivery_eligible: false,
                    }) | CleanupOutcome::Returned(CleanupObservation::Watch { watching: false })
                )
            });
        if returned_local_retirement && reporting_error.is_none() {
            Ok(())
        } else {
            Err(Box::new(MatrixCleanupFailure {
                report,
                reporting_error,
            }))
        }
    }
}

#[derive(Debug)]
struct MatrixCleanupFailure {
    report: MatrixCleanupReport,
    reporting_error: Option<std::io::Error>,
}
impl std::fmt::Display for MatrixCleanupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "matrix resource cleanup incomplete: {:?}; metadata reporting error: {:?}",
            self.report,
            self.reporting_error.as_ref().map(std::io::Error::kind)
        )
    }
}
impl std::error::Error for MatrixCleanupFailure {}

#[derive(Debug)]
pub(super) struct MatrixCleanupReport {
    pub(super) history: MatrixCreationHistory,
    pub(super) unresolved: Vec<UnresolvedCreation>,
    pub(super) attempts: Vec<CleanupAttempt>,
}

#[derive(Debug)]
pub(super) struct CleanupAttempt {
    pub(super) action: CleanupAction,
    pub(super) outcome: CleanupOutcome,
}

#[derive(Debug)]
pub(super) enum CleanupAction {
    CancelWake {
        operation_id: OperationId,
        wakeup_id: WakeupId,
    },
    DisableSchedule {
        operation_id: OperationId,
        schedule_id: ScheduleId,
    },
    Unsubscribe {
        actor: Identity,
        root_message_id: ThreadId,
    },
    Unwatch {
        actor: Identity,
        root_message_id: ThreadId,
    },
}

#[derive(Debug)]
pub(super) enum CleanupOutcome {
    /// A matching public success reply, not a task join or independent enforcement proof.
    Returned(CleanupObservation),
    /// A reported RPC rejection; Board rejection can follow a committed local mutation.
    Rejected(ReportedCleanupFailure),
    Unknown(CleanupUncertainty),
}

#[derive(Debug)]
pub(super) enum CleanupObservation {
    Wake {
        state: WakeState,
        next_due_at: Option<ObservationTimestamp>,
        pending_delivery_id: Option<DeliveryId>,
        discarded_delivery_ids: Vec<DeliveryId>,
        retained_deliveries: Vec<RetainedDeliveryObservation>,
    },
    Schedule {
        change_id: ChangeId,
        enabled: bool,
        next_due_at: Option<ObservationTimestamp>,
        active_run_id: Option<RunId>,
        waiting_run_id: Option<RunId>,
    },
    Subscription {
        delivery_eligible: bool,
    },
    Watch {
        watching: bool,
    },
}

#[derive(Debug)]
pub(super) struct RetainedDeliveryObservation {
    pub(super) delivery_id: DeliveryId,
    pub(super) disposition: DeliveryDisposition,
}

#[derive(Debug)]
pub(super) enum ReportedCleanupFailure {
    Wake {
        kind: WakeCleanupFailureKind,
        stage: WakeFailureStage,
        operation_id: Option<OperationId>,
        wakeup_id: Option<WakeupId>,
        mutation: LocalMutationState,
    },
    Schedule {
        kind: ScheduleFailureKind,
        stage: ScheduleFailureStage,
        operation_id: Option<OperationId>,
        schedule_id: Option<ScheduleId>,
        current_change_id: Option<ChangeId>,
        effect: ReportedCleanupEffect,
    },
    Board {
        kind: BoardFailureKind,
        stage: BoardFailureStage,
    },
}

/// The wire InvalidField variant also carries arbitrary strings; retain its closed tag only.
pub(super) enum WakeCleanupFailureKind {
    Refused(collaboration_protocol::WakeRefusal),
    InvalidField,
    OperationConflict,
    AutomationUnavailable,
    ResourceNotFound,
    InvalidRecord,
    LifecycleConflict,
    Overloaded,
    RemoteUnavailable,
    DeadlineExpired,
}

impl std::fmt::Debug for WakeCleanupFailureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(reason) => formatter.debug_tuple("Refused").field(reason).finish(),
            Self::InvalidField => formatter.write_str("InvalidField"),
            Self::OperationConflict => formatter.write_str("OperationConflict"),
            Self::AutomationUnavailable => formatter.write_str("AutomationUnavailable"),
            Self::ResourceNotFound => formatter.write_str("ResourceNotFound"),
            Self::InvalidRecord => formatter.write_str("InvalidRecord"),
            Self::LifecycleConflict => formatter.write_str("LifecycleConflict"),
            Self::Overloaded => formatter.write_str("Overloaded"),
            Self::RemoteUnavailable => formatter.write_str("RemoteUnavailable"),
            Self::DeadlineExpired => formatter.write_str("DeadlineExpired"),
        }
    }
}

#[derive(Debug)]
pub(super) enum ReportedCleanupEffect {
    Local(LocalMutationState),
    /// Retain the reported category without copying route payloads or claiming cessation.
    Route,
}

#[derive(Debug)]
pub(super) enum CleanupUncertainty {
    Client(ClientFailureKind),
    BoardOutcomeUnknown,
    UnexpectedWaitOutcomeUnknown,
    ResponseIdentityMismatch,
    ReadbackUnavailable,
}

#[derive(Debug)]
pub(super) enum ClientFailureKind {
    InvalidRequest,
    UnsupportedCapability,
    Discovery,
    Transport(std::io::ErrorKind),
    Protocol,
    Timeout,
    RpcRejected { code: i64 },
    Overloaded,
}

/// Consume the plan once. No response, rejection or unknown outcome causes a second attempt.
pub(super) async fn cleanup_owned_resources(
    client: &CollaborationClient,
    plan: MatrixCleanupPlan,
) -> MatrixCleanupReport {
    let MatrixCleanupPlan {
        intents,
        unresolved,
        history,
    } = plan;
    let mut attempts = Vec::with_capacity(intents.len());
    for intent in intents {
        // The owned action, including any generated operation ID, exists before dispatch.
        let action = prepare_cleanup_action(intent);
        let outcome = execute_cleanup_action(client, &action).await;
        attempts.push(CleanupAttempt { action, outcome });
    }
    MatrixCleanupReport {
        history,
        unresolved,
        attempts,
    }
}

fn prepare_cleanup_action(intent: CleanupIntent) -> CleanupAction {
    match intent {
        CleanupIntent::CancelWake(wakeup_id) => CleanupAction::CancelWake {
            operation_id: OperationId::generate(),
            wakeup_id,
        },
        CleanupIntent::DisableSchedule(schedule_id) => CleanupAction::DisableSchedule {
            operation_id: OperationId::generate(),
            schedule_id,
        },
        CleanupIntent::Unsubscribe {
            actor,
            root_message_id,
        } => CleanupAction::Unsubscribe {
            actor,
            root_message_id,
        },
        CleanupIntent::Unwatch {
            actor,
            root_message_id,
        } => CleanupAction::Unwatch {
            actor,
            root_message_id,
        },
    }
}

async fn execute_cleanup_action(
    client: &CollaborationClient,
    action: &CleanupAction,
) -> CleanupOutcome {
    match action {
        CleanupAction::CancelWake {
            operation_id,
            wakeup_id,
        } => {
            match client
                .cancel_wakeup(WakeMutationRequest {
                    operation_id: operation_id.clone(),
                    wakeup_id: wakeup_id.clone(),
                })
                .await
            {
                Ok(result) => project_wake_result(wakeup_id, result),
                Err(error) => project_wake_failure(error),
            }
        }
        CleanupAction::DisableSchedule {
            operation_id,
            schedule_id,
        } => {
            match client
                .disable_schedule(ScheduleEnableRequest {
                    operation_id: operation_id.clone(),
                    schedule_id: schedule_id.clone(),
                })
                .await
            {
                Ok(result) if &result.schedule_id == schedule_id => {
                    CleanupOutcome::Returned(CleanupObservation::Schedule {
                        change_id: result.change_id,
                        enabled: result.definition.enabled,
                        next_due_at: result.next_due_at,
                        active_run_id: result.active_run_id,
                        waiting_run_id: result.waiting_run_id,
                    })
                }
                Ok(_) => CleanupOutcome::Unknown(CleanupUncertainty::ResponseIdentityMismatch),
                Err(error) => project_schedule_failure(error),
            }
        }
        CleanupAction::Unsubscribe {
            actor,
            root_message_id,
        } => {
            let scope = SubscriptionScope::thread(root_message_id.clone());
            match client
                .thread_unsubscribe(ThreadUnsubscribeRequest {
                    request_id: message_board::RequestId::generate(),
                    protocol_version: message_board::ProtocolVersion::CURRENT,
                    acting_for: None,
                    actor: actor.clone(),
                    scope: scope.clone().into(),
                })
                .await
            {
                Ok(_) => read_subscription_cleanup(client, actor, root_message_id).await,
                Err(error) => project_board_failure(error),
            }
        }
        CleanupAction::Unwatch {
            actor,
            root_message_id,
        } => {
            match client
                .thread_unwatch(ThreadUnwatchRequest {
                    request_id: message_board::RequestId::generate(),
                    protocol_version: message_board::ProtocolVersion::CURRENT,
                    thread: root_message_id.clone().into(),
                    actor: actor.clone(),
                    acting_for: None,
                })
                .await
            {
                Ok(_) => {
                    match client
                        .thread_show(message_board::ThreadShowRequest {
                            thread: root_message_id.clone().into(),
                            reader: message_board::ReadActor::Identified {
                                reader: actor.clone(),
                            },
                            include_redacted: false,
                        })
                        .await
                    {
                        Ok(result) if &result.thread.thread_id == root_message_id => {
                            match result.watch_status {
                                Some(watch) => {
                                    CleanupOutcome::Returned(CleanupObservation::Watch {
                                        watching: watch.watching,
                                    })
                                }
                                None => {
                                    CleanupOutcome::Unknown(CleanupUncertainty::ReadbackUnavailable)
                                }
                            }
                        }
                        Ok(_) => {
                            CleanupOutcome::Unknown(CleanupUncertainty::ResponseIdentityMismatch)
                        }
                        Err(error) => project_board_failure(error),
                    }
                }
                Err(error) => project_board_failure(error),
            }
        }
    }
}

async fn read_subscription_cleanup(
    client: &CollaborationClient,
    actor: &Identity,
    thread: &ThreadId,
) -> CleanupOutcome {
    match client
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: actor.clone(),
        })
        .await
    {
        Ok(snapshot) => {
            if !snapshot.unavailable.is_empty() {
                return CleanupOutcome::Unknown(CleanupUncertainty::ReadbackUnavailable);
            }
            let scope = SubscriptionScope::thread(thread.clone());
            let mut matching = snapshot
                .subscriptions
                .iter()
                .filter(|record| record.scope == scope);
            let delivery_eligible = matching.next().is_some_and(|record| {
                matches!(
                    record.state,
                    ThreadSubscriptionState::Active | ThreadSubscriptionState::Draining
                )
            });
            if matching.next().is_some() {
                return CleanupOutcome::Unknown(CleanupUncertainty::ResponseIdentityMismatch);
            }
            CleanupOutcome::Returned(CleanupObservation::Subscription { delivery_eligible })
        }
        Err(error) => project_board_failure(error),
    }
}

fn project_wake_result(wakeup_id: &WakeupId, result: WakeMutationResult) -> CleanupOutcome {
    match result {
        result if &result.wakeup.definition.wakeup_id == wakeup_id => {
            CleanupOutcome::Returned(CleanupObservation::Wake {
                state: result.wakeup.state,
                next_due_at: result.wakeup.next_due_at,
                pending_delivery_id: result.wakeup.pending_delivery_id,
                discarded_delivery_ids: result.discarded_delivery_ids,
                retained_deliveries: result
                    .dispatched_deliveries
                    .into_iter()
                    .map(|delivery| RetainedDeliveryObservation {
                        delivery_id: delivery.delivery_id,
                        disposition: delivery.disposition,
                    })
                    .collect(),
            })
        }
        _ => CleanupOutcome::Unknown(CleanupUncertainty::ResponseIdentityMismatch),
    }
}

fn project_wake_failure(error: WakeClientError) -> CleanupOutcome {
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
        WakeClientError::Connection(error) => project_client_failure(error),
    }
}

fn project_schedule_failure(error: ScheduleClientError) -> CleanupOutcome {
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
        ScheduleClientError::Connection(error) => project_client_failure(error),
    }
}

fn project_board_failure(error: BoardClientError) -> CleanupOutcome {
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

fn project_client_failure(error: ClientError) -> CleanupOutcome {
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

#[cfg(test)]
#[path = "owned_resource_cleanup_tests.rs"]
mod tests;
