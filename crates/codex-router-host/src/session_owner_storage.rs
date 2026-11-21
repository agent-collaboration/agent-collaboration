//! Execute owner store effects through its session's ordered lane.

use crate::session_store_lane::{
    LaneDispatchKind, SessionStoreLane, SessionStoreOfferError, SessionStoreSettlement,
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
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Clone)]
pub struct SessionOwnerStorage {
    lane: SessionStoreLane,
    tasks: TaskTracker,
    setup_reads: Arc<Mutex<HashMap<LoadAttempt, Box<ProviderSessionRecord>>>>,
    explicit_setups: Arc<Mutex<HashMap<LoadAttempt, Box<ProviderSessionRecord>>>>,
    delivery_restores: Arc<Mutex<HashSet<LoadAttempt>>>,
    explicit_modes: Arc<Mutex<HashMap<SettingApplication, String>>>,
    parked_dispatches: Arc<Mutex<HashMap<uuid::Uuid, CancellationToken>>>,
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
            parked_dispatches: Arc::default(),
        }
    }

    pub(crate) fn task_tracker(&self) -> TaskTracker {
        self.tasks.clone()
    }

    pub(crate) fn release_dispatch_wait(&self, reply: &SessionOwnerReply) {
        let input = match reply {
            SessionOwnerReply::InputUnsent { input, .. }
            | SessionOwnerReply::InputFinished { input, .. }
            | SessionOwnerReply::PromptCompleted { input, .. }
            | SessionOwnerReply::PromptFailed { input, .. }
            | SessionOwnerReply::PromptCancelUnconfirmed { input }
            | SessionOwnerReply::SteerInvalidReply { input }
            | SessionOwnerReply::SteerStarted { input, .. } => input,
            _ => return,
        };
        if let Some(wait) = lock(&self.parked_dispatches).remove(&input.operation) {
            wait.cancel();
        }
    }

    pub(crate) fn stop_dispatch_waits(&self) {
        for (_, wait) in lock(&self.parked_dispatches).drain() {
            wait.cancel();
        }
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
                self.report_dispatch(address, input, LaneDispatchKind::Prompt, completions)
            }
            SessionStoreRequest::MarkQueuedDispatch(input) => {
                self.report_dispatch(address, input, LaneDispatchKind::QueuedPrompt, completions)
            }
            SessionStoreRequest::MarkSteerDispatch(input) => {
                self.report_dispatch(address, input, LaneDispatchKind::Steer, completions)
            }
            SessionStoreRequest::UndoDispatch(input) => {
                let operation = operation_id(input.operation)?;
                let undo_input = input.clone();
                self.report(
                    address,
                    || {
                        self.lane.offer_dispatch(
                            owner,
                            operation,
                            undo_input,
                            LaneDispatchKind::Undo,
                        )
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
                let repair_input = Box::new(input.clone());
                self.report(
                    address,
                    || {
                        self.lane.offer(
                            owner,
                            SessionStoreWrite::ReconcileLateCancelledPrompt {
                                operation,
                                input: repair_input,
                            },
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

    fn report_dispatch(
        &self,
        address: OwnerAddress,
        input: OwnerInput,
        kind: LaneDispatchKind,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let completion = completions.reserve(address)?;
        let operation = operation_id(input.operation)?;
        let lane = self.lane.clone();
        let mut changes = lane.subscribe_dispatch_eligibility();
        let offer =
            lane.offer_dispatch(address.incarnation, operation.clone(), input.clone(), kind);
        if kind == LaneDispatchKind::QueuedPrompt
            && matches!(offer, Err(SessionStoreOfferError::DispatchBusy { .. }))
            && let Some(mut changes) = changes.take()
        {
            let attempt = uuid::Uuid::now_v7();
            let cancelled = CancellationToken::new();
            if let Some(previous) =
                lock(&self.parked_dispatches).insert(input.operation, cancelled.clone())
            {
                previous.cancel();
            }
            #[cfg(any(test, feature = "test-support"))]
            {
                use crate::binding_store_fixture::BindingFixtureWait;
                lane.notice_fixture_wait(BindingFixtureWait::QueuedDispatchParked);
                lane.notice_fixture_wait(BindingFixtureWait::QueuedDispatchObserved(
                    operation.clone(),
                ));
            }
            let ready_input = input.clone();
            self.tasks.spawn(async move {
                let ready = async {
                    // Subscribe before the original decision. An intervening
                    // release is visible either here or through changed().
                    while lane.waits_for_live_dispatch_blocker(&operation, kind) {
                        if changes.changed().await.is_err() {
                            return false;
                        }
                    }
                    true
                };
                tokio::select! {
                    biased;
                    () = cancelled.cancelled() => {},
                    ready = ready => if ready {
                        completion.send(OwnerEvent::Store(
                            SessionStoreResult::QueuedDispatchReady { input: ready_input, attempt }
                        ));
                    },
                }
            });
            return Ok(Some(OwnerEvent::Store(
                SessionStoreResult::QueuedDispatchBusy { input, attempt },
            )));
        }
        if let Err(SessionStoreOfferError::EarlierOutcomeUnknown { operation }) = offer {
            return Ok(Some(OwnerEvent::Store(
                SessionStoreResult::DispatchBlockedByUnknown { input, operation },
            )));
        }
        let clock = lane.dispatch_clock();
        let deadline = clock
            .monotonic_now()
            .after(crate::session_store_lane::STORE_WAIT);
        self.tasks.spawn(async move {
            let mut offer = offer;
            if matches!(
                offer,
                Err(SessionStoreOfferError::DispatchBusy { pending: true, .. })
            ) && let Some(mut changes) = changes
            {
                let waiting = async {
                    loop {
                        changes
                            .changed()
                            .await
                            .map_err(|_| SessionStoreOfferError::Stopped)?;
                        if clock.monotonic_now() >= deadline {
                            return Err(SessionStoreOfferError::Stopped);
                        }
                        let reply = lane.offer_dispatch(
                            address.incarnation,
                            operation.clone(),
                            input.clone(),
                            kind,
                        );
                        if !matches!(
                            reply,
                            Err(SessionStoreOfferError::DispatchBusy { pending: true, .. })
                        ) {
                            return reply;
                        }
                    }
                };
                offer = router_clock::timeout_at(clock.as_ref(), deadline, waiting)
                    .await
                    .unwrap_or(Err(SessionStoreOfferError::Stopped));
            }
            let outcome = match offer {
                Ok(ticket) => store_effect(ticket.settlement().await),
                Err(SessionStoreOfferError::EarlierOutcomeUnknown { operation }) => {
                    completion.send(OwnerEvent::Store(
                        SessionStoreResult::DispatchBlockedByUnknown { input, operation },
                    ));
                    return;
                }
                Err(_) => EffectOutcome::Failed,
            };
            let result = if kind == LaneDispatchKind::Steer {
                SessionStoreResult::SteerDispatch { input, outcome }
            } else {
                SessionStoreResult::Dispatch { input, outcome }
            };
            completion.send(OwnerEvent::Store(result));
        });
        Ok(None)
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
                    SessionStoreOfferError::Replaced
                    | SessionStoreOfferError::Stopped
                    | SessionStoreOfferError::DispatchBusy { .. } => SessionStoreWaitError::Stopped,
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
        cancelled_input: match reply {
            SessionOwnerReply::PromptCancelUnconfirmed { input } => Some(Box::new(input.clone())),
            _ => None,
        },
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
