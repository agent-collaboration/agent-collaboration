//! Execute owner store effects through its session's ordered lane.

use crate::session_store_lane::{
    MarkedInput, SessionStoreLane, SessionStoreOfferError, SessionStoreSettlement,
    SessionStoreTicket, SessionStoreWaitError, SessionStoreWrite,
};
use crate::{binding_dispatch_permit::UnknownRecovery, session_close_evidence::classify_close};
use acp_client_runtime::session_owner_task::{OwnerCompletionPort, OwnerTaskError};
use collaboration_protocol::{
    OperationId, ProviderOperationEffect, ProviderPromptStopReason, ProviderReconciliationState,
};
use collaboration_runtime::ProviderSessionRecord;
use provider_session_machines::{
    ProviderSettingKind,
    session_lifecycle_machine::LoadAttempt,
    session_owner::{
        EffectOutcome, OwnerAddress, OwnerEvent, OwnerInput, OwnerSetting, RemoteEffect,
        RestoreDisposition, SessionOwnerReply, SessionStoreRequest, SessionStoreResult,
    },
    session_setting_machine::SettingApplication,
};
use session_event_model::StopReason;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};
use tokio_util::task::TaskTracker;

#[derive(Clone)]
pub struct SessionOwnerStorage {
    lane: SessionStoreLane,
    tasks: TaskTracker,
    setup_reads: Arc<Mutex<HashMap<LoadAttempt, Box<ProviderSessionRecord>>>>,
    explicit_setups: Arc<Mutex<HashMap<LoadAttempt, Box<ProviderSessionRecord>>>>,
    delivery_restores: Arc<Mutex<HashSet<LoadAttempt>>>,
    explicit_modes: Arc<Mutex<HashMap<SettingApplication, String>>>,
}

impl SessionOwnerStorage {
    pub fn new(lane: SessionStoreLane, tasks: TaskTracker) -> Self {
        Self {
            lane,
            tasks,
            setup_reads: Arc::default(),
            explicit_setups: Arc::default(),
            delivery_restores: Arc::default(),
            explicit_modes: Arc::default(),
        }
    }

    pub(crate) fn task_tracker(&self) -> TaskTracker {
        self.tasks.clone()
    }

    pub fn take_setup(&self, attempt: LoadAttempt) -> Option<Box<ProviderSessionRecord>> {
        lock(&self.setup_reads).remove(&attempt)
    }

    pub fn expect_explicit_restore(
        &self,
        attempt: LoadAttempt,
        record: Box<ProviderSessionRecord>,
    ) {
        lock(&self.explicit_setups).insert(attempt, record);
    }

    pub fn explicit_working_directory(&self, attempt: LoadAttempt) -> Option<std::path::PathBuf> {
        lock(&self.explicit_setups)
            .get(&attempt)
            .map(|record| std::path::PathBuf::from(String::from(record.working_directory.clone())))
    }

    pub fn forget_restore(&self, attempt: LoadAttempt) {
        lock(&self.explicit_setups).remove(&attempt);
        lock(&self.delivery_restores).remove(&attempt);
        lock(&self.setup_reads).remove(&attempt);
    }

    pub fn expect_delivery_restore(&self, attempt: LoadAttempt) {
        lock(&self.delivery_restores).insert(attempt);
    }

    pub fn expect_confirmed_mode(&self, setting: OwnerSetting, asked: String) {
        if setting.kind == ProviderSettingKind::Mode {
            lock(&self.explicit_modes).insert(setting.change.attempt, asked);
        }
    }

    pub fn forget_setting(&self, setting: OwnerSetting) {
        lock(&self.explicit_modes).remove(&setting.change.attempt);
    }

    pub(crate) fn defer_queued_admission(
        &self,
        owner: provider_session_machines::session_owner::OwnerIncarnation,
        admission: collaboration_runtime::ProviderOperationAdmission,
    ) -> Result<(), SessionStoreOfferError> {
        self.lane.defer_queued_admission(owner, admission)
    }

    pub(crate) fn release_queued_admission(
        &self,
        owner: provider_session_machines::session_owner::OwnerIncarnation,
        operation: &OperationId,
    ) {
        self.lane.release_queued_admission(owner, operation);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn notice_queued_setting(&self) {
        self.lane
            .notice_fixture_wait(crate::binding_store_fixture::BindingFixtureWait::ExplicitSetting);
    }

    pub fn execute(
        &self,
        address: OwnerAddress,
        request: SessionStoreRequest,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let owner = address.incarnation;
        match request {
            SessionStoreRequest::ReadRestoreSetup(run) => {
                let setup_reads = Arc::clone(&self.setup_reads);
                let explicit_setups = Arc::clone(&self.explicit_setups);
                self.report(
                    address,
                    || self.lane.read_setup(owner),
                    completions,
                    move |receipt| {
                        let outcome = match receipt {
                            Ok(SessionStoreSettlement::Setup(Some(setup))) => {
                                if let Some(explicit) = lock(&explicit_setups).get_mut(&run.attempt)
                                {
                                    explicit.resume = setup.resume.clone();
                                }
                                lock(&setup_reads).insert(run.attempt, setup);
                                EffectOutcome::Applied
                            }
                            Ok(SessionStoreSettlement::Setup(None)) => {
                                let explicit = lock(&explicit_setups).get(&run.attempt).cloned();
                                if let Some(explicit) = explicit {
                                    lock(&setup_reads).insert(run.attempt, explicit);
                                    EffectOutcome::Applied
                                } else {
                                    EffectOutcome::Failed
                                }
                            }
                            Ok(_) => EffectOutcome::Failed,
                            Err(error) => store_error(&error),
                        };
                        SessionStoreResult::RestoreSetup { run, outcome }
                    },
                )
            }
            SessionStoreRequest::MarkDispatch(input) => {
                let mark = marked_input(&input)?;
                self.report(
                    address,
                    || {
                        self.lane
                            .offer(owner, SessionStoreWrite::MarkDispatch(mark))
                    },
                    completions,
                    move |receipt| match receipt {
                        Err(SessionStoreWaitError::EarlierOutcomeUnknown { operation }) => {
                            SessionStoreResult::DispatchBlockedByUnknown { input, operation }
                        }
                        receipt => SessionStoreResult::Dispatch {
                            input,
                            outcome: store_effect(receipt),
                        },
                    },
                )
            }
            SessionStoreRequest::MarkSteerDispatch(input) => {
                let mark = marked_input(&input)?;
                self.report(
                    address,
                    || {
                        self.lane
                            .offer(owner, SessionStoreWrite::MarkDispatch(mark))
                    },
                    completions,
                    move |receipt| match receipt {
                        Err(SessionStoreWaitError::EarlierOutcomeUnknown { operation }) => {
                            SessionStoreResult::DispatchBlockedByUnknown { input, operation }
                        }
                        receipt => SessionStoreResult::SteerDispatch {
                            input,
                            outcome: store_effect(receipt),
                        },
                    },
                )
            }
            SessionStoreRequest::UndoDispatch(input) => {
                let mark = marked_input(&input)?;
                self.report(
                    address,
                    || {
                        self.lane
                            .offer(owner, SessionStoreWrite::UndoDispatch(mark))
                    },
                    completions,
                    move |receipt| SessionStoreResult::UndoDispatch {
                        input,
                        outcome: store_effect(receipt),
                    },
                )
            }
            SessionStoreRequest::ReconcileLateCancelledPrompt(input) => {
                let operation = operation_id(input.operation)?;
                self.report(
                    address,
                    || {
                        self.lane.offer(
                            owner,
                            SessionStoreWrite::ReconcileLateCancelledPrompt(operation),
                        )
                    },
                    completions,
                    move |receipt| SessionStoreResult::LateCancelledPromptReconciled {
                        input,
                        outcome: store_effect(receipt),
                    },
                )
            }
            SessionStoreRequest::RecordTerminal { write, reply } => {
                if let SessionOwnerReply::RestoreFinished(result) = reply.as_ref()
                    && lock(&self.delivery_restores).remove(&result.attempt.attempt)
                {
                    return Ok(Some(OwnerEvent::Store(SessionStoreResult::Terminal {
                        write,
                        outcome: EffectOutcome::Applied,
                    })));
                }
                let persisted_restore =
                    if let SessionOwnerReply::RestoreFinished(result) = reply.as_ref() {
                        let explicit = lock(&self.explicit_setups).remove(&result.attempt.attempt);
                        if matches!(result.disposition, RestoreDisposition::Restored { .. }) {
                            explicit
                                .map(|record| {
                                    operation_id(result.attempt.attempt.0).map(|operation| {
                                        SessionStoreWrite::ActivatedSession { operation, record }
                                    })
                                })
                                .transpose()?
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                let Some(request) = persisted_restore.or(terminal_write(&reply)?) else {
                    return Ok(Some(OwnerEvent::Store(SessionStoreResult::Terminal {
                        write,
                        outcome: EffectOutcome::Applied,
                    })));
                };
                self.report(
                    address,
                    || self.lane.offer(owner, request),
                    completions,
                    move |receipt| SessionStoreResult::Terminal {
                        write,
                        outcome: store_effect(receipt),
                    },
                )
            }
            SessionStoreRequest::RecordConfirmedSetting { setting, value } => {
                let asked = lock(&self.explicit_modes).remove(&setting.change.attempt);
                if setting.kind == ProviderSettingKind::Mode
                    && asked.as_deref() == Some(value.0.as_str())
                {
                    self.report(
                        address,
                        || self.lane.offer(owner, SessionStoreWrite::SaveMode(value.0)),
                        completions,
                        move |receipt| SessionStoreResult::ConfirmedSetting {
                            setting,
                            outcome: store_effect(receipt),
                        },
                    )
                } else {
                    Ok(Some(OwnerEvent::Store(
                        SessionStoreResult::ConfirmedSetting {
                            setting,
                            outcome: EffectOutcome::Failed,
                        },
                    )))
                }
            }
        }
    }

    fn report(
        &self,
        address: OwnerAddress,
        offer: impl FnOnce() -> Result<SessionStoreTicket, SessionStoreOfferError>,
        completions: &OwnerCompletionPort,
        outcome: impl FnOnce(
            Result<SessionStoreSettlement, SessionStoreWaitError>,
        ) -> SessionStoreResult
        + Send
        + 'static,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let completion = completions.reserve(address)?;
        let ticket = match offer() {
            Ok(ticket) => ticket,
            Err(error) => {
                return Ok(Some(OwnerEvent::Store(outcome(Err(match error {
                    SessionStoreOfferError::EarlierOutcomeUnknown { operation } => {
                        SessionStoreWaitError::EarlierOutcomeUnknown { operation }
                    }
                    SessionStoreOfferError::Replaced | SessionStoreOfferError::Stopped => {
                        SessionStoreWaitError::Stopped
                    }
                })))));
            }
        };
        self.tasks.spawn(async move {
            completion.send(OwnerEvent::Store(outcome(ticket.settlement().await)));
        });
        Ok(None)
    }
}

fn lock<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn marked_input(input: &OwnerInput) -> Result<MarkedInput, OwnerTaskError> {
    Ok(MarkedInput {
        input: input.input.clone(),
        operation: operation_id(input.operation)?,
    })
}

fn operation_id(id: uuid::Uuid) -> Result<OperationId, OwnerTaskError> {
    OperationId::try_from(id.to_string()).map_err(|_| OwnerTaskError::ExecutorStopped)
}

fn store_effect(result: Result<SessionStoreSettlement, SessionStoreWaitError>) -> EffectOutcome {
    result.map_or_else(|error| store_error(&error), |_| EffectOutcome::Applied)
}

fn store_error(error: &SessionStoreWaitError) -> EffectOutcome {
    match error {
        SessionStoreWaitError::Elapsed
        | SessionStoreWaitError::Store(
            collaboration_runtime::ProviderOperationStoreError::Database(_),
        ) => EffectOutcome::Unknown,
        SessionStoreWaitError::Stopped
        | SessionStoreWaitError::EarlierOutcomeUnknown { .. }
        | SessionStoreWaitError::Store(_) => EffectOutcome::Failed,
    }
}

fn terminal_write(reply: &SessionOwnerReply) -> Result<Option<SessionStoreWrite>, OwnerTaskError> {
    let recovery = match reply {
        SessionOwnerReply::CloseFinished { outcome, .. } if outcome.locally_retired() => {
            UnknownRecovery::RetiredClose
        }
        SessionOwnerReply::RestoreFinished(_) => UnknownRecovery::RetiredRestore,
        _ => UnknownRecovery::Blocks,
    };
    let (operation, effect, reconciliation, stop_reason) = match reply {
        SessionOwnerReply::CloseFinished { run, outcome } => {
            let close = classify_close(*outcome);
            (
                operation_id(run.attempt.0)?,
                close.effect,
                close.reconciliation,
                None,
            )
        }
        SessionOwnerReply::RestoreFinished(result) => {
            let remote = match result.disposition {
                RestoreDisposition::Restored { .. } => RemoteEffect::Applied,
                RestoreDisposition::Retired { remote, .. } => remote,
            };
            let (effect, reconciliation) = terminal_evidence(remote);
            (
                operation_id(result.attempt.attempt.0)?,
                effect,
                reconciliation,
                None,
            )
        }
        SessionOwnerReply::InputFinished { input, remote }
        | SessionOwnerReply::PromptFailed { input, remote, .. } => {
            let (effect, reconciliation) = terminal_evidence(*remote);
            (operation_id(input.operation)?, effect, reconciliation, None)
        }
        SessionOwnerReply::PromptCancelUnconfirmed { input }
        | SessionOwnerReply::SteerInvalidReply { input } => (
            operation_id(input.operation)?,
            ProviderOperationEffect::Unknown,
            ProviderReconciliationState::Unresolved,
            None,
        ),
        SessionOwnerReply::InputUnsent { input, .. } => (
            operation_id(input.operation)?,
            ProviderOperationEffect::None,
            ProviderReconciliationState::Confirmed,
            None,
        ),
        SessionOwnerReply::SteerStarted { input, .. } => (
            operation_id(input.operation)?,
            ProviderOperationEffect::Applied,
            ProviderReconciliationState::Confirmed,
            None,
        ),
        SessionOwnerReply::PromptCompleted { input, completion } => (
            operation_id(input.operation)?,
            ProviderOperationEffect::Applied,
            ProviderReconciliationState::Confirmed,
            (!matches!(completion.stop_reason, StopReason::Unknown(_)))
                .then(|| stop_reason(&completion.stop_reason)),
        ),
        SessionOwnerReply::SettingFinished { .. }
        | SessionOwnerReply::SettingKept { .. }
        | SessionOwnerReply::SettingNotSent { .. }
        | SessionOwnerReply::ModelFastFinished { .. } => {
            return Ok(None);
        }
    };
    Ok(Some(SessionStoreWrite::Terminal {
        operation,
        effect,
        reconciliation,
        stop_reason,
        recovery,
    }))
}

fn terminal_evidence(
    remote: RemoteEffect,
) -> (ProviderOperationEffect, ProviderReconciliationState) {
    match remote {
        RemoteEffect::None => (
            ProviderOperationEffect::None,
            ProviderReconciliationState::Confirmed,
        ),
        RemoteEffect::Applied => (
            ProviderOperationEffect::Applied,
            ProviderReconciliationState::Confirmed,
        ),
        RemoteEffect::Unknown => (
            ProviderOperationEffect::Unknown,
            ProviderReconciliationState::Unresolved,
        ),
    }
}

pub(crate) fn stop_reason(reason: &StopReason) -> ProviderPromptStopReason {
    match reason {
        StopReason::EndTurn => ProviderPromptStopReason::EndTurn,
        StopReason::MaxTokens => ProviderPromptStopReason::MaxTokens,
        StopReason::MaxTurnRequests => ProviderPromptStopReason::MaxTurnRequests,
        StopReason::Refusal => ProviderPromptStopReason::Refusal,
        StopReason::Cancelled => ProviderPromptStopReason::Cancelled,
        StopReason::Unknown(reason) => ProviderPromptStopReason::Unknown(reason.clone()),
    }
}
