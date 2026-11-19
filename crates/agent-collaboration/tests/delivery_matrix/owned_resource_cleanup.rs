//! One-pass Unsubscribe/Unwatch cleanup followed by Board readback of each owned pair.
#[path = "owned_resource_cleanup_readback.rs"]
pub(super) mod readback;
use super::owned_resources::{CleanupIntent, MatrixCleanupPlan};
use collaboration_client::{
    BoardClientError, ClientError, CollaborationClient,
    board::{
        BoardFailureKind, BoardFailureStage, Identity, SubscriptionScope, ThreadId,
        ThreadUnwatchRequest,
    },
    protocol::{ThreadSubscriptionState, ThreadSubscriptionsRequest, ThreadUnsubscribeRequest},
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
        let returned_local_retirement = report.attempts.iter().all(|attempt| {
            matches!(
                &attempt.outcome,
                CleanupOutcome::Returned(CleanupObservation::Subscription {
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
    pub(super) attempts: Vec<CleanupAttempt>,
}

#[derive(Debug)]
pub(super) struct CleanupAttempt {
    #[expect(dead_code, reason = "Read only through the Debug cleanup report.")]
    pub(super) action: CleanupAction,
    pub(super) outcome: CleanupOutcome,
}

#[derive(Debug)]
pub(super) enum CleanupAction {
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
    Subscription { delivery_eligible: bool },
    Watch { watching: bool },
}

#[derive(Debug)]
pub(super) enum ReportedCleanupFailure {
    Board {
        kind: BoardFailureKind,
        stage: BoardFailureStage,
    },
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
        history: _,
    } = plan;
    let mut attempts = Vec::with_capacity(intents.len());
    for intent in intents {
        // The owned action exists before dispatch.
        let action = prepare_cleanup_action(intent);
        let outcome = execute_cleanup_action(client, &action).await;
        attempts.push(CleanupAttempt { action, outcome });
    }
    MatrixCleanupReport { attempts }
}

fn prepare_cleanup_action(intent: CleanupIntent) -> CleanupAction {
    match intent {
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
