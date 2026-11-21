//! Ordered operation and saved-setup stores for a single provider session.

use crate::binding_dispatch_permit::{BindingUncertainty, UnknownRecovery, lock};
use crate::provider_operation_custody::{CancelledPromptIdentity, CustodyEnd, LaneWriteCustody};
use collaboration_protocol::{
    ConversationBindingIdentity, OperationId, ProviderOperationEffect, ProviderOperationKind,
    ProviderOperationStage, ProviderPromptStopReason, ProviderReconciliationState, SessionRef,
};
use collaboration_runtime::{
    ProviderOperationAdmission, ProviderOperationRecord, ProviderOperationStore,
    ProviderOperationStoreError, ProviderSessionRecord,
};
use provider_session_machines::session_owner::{OwnerIncarnation, OwnerInput};
use router_clock::{ClockInstant, RuntimeClock};
use session_event_model::InputId;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, Notify, oneshot};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

#[path = "session_store_dispatch.rs"]
mod dispatch;
pub(crate) use dispatch::LaneDispatchKind;
use dispatch::{LaneDispatchOffer, LaneDispatchWrite};

pub(crate) const STORE_WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkedInput {
    pub input: InputId,
    pub operation: OperationId,
}

#[derive(Debug)]
pub enum SessionStoreWrite {
    MarkDispatch(MarkedInput),
    UndoDispatch(MarkedInput),
    ReconcileLateCancelledPrompt {
        operation: OperationId,
        input: Box<OwnerInput>,
    },
    Terminal {
        operation: OperationId,
        effect: ProviderOperationEffect,
        reconciliation: ProviderReconciliationState,
        stop_reason: Option<ProviderPromptStopReason>,
        recovery: UnknownRecovery,
        cancelled_input: Option<Box<OwnerInput>>,
    },
    ActivatedSession {
        operation: OperationId,
        record: Box<ProviderSessionRecord>,
    },
    SaveMode(String),
}

#[derive(Debug)]
pub enum SessionStoreSettlement {
    Operation(Box<ProviderOperationRecord>),
    Setup(Option<Box<ProviderSessionRecord>>),
    ModeSaved,
    Activated,
    MarkNeverWritten,
    HandedOver,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SessionStoreOfferError {
    #[error("this session has a different current owner")]
    Replaced,
    #[error("the session's store lane stopped")]
    Stopped,
    #[error("not sent: an earlier outcome is unknown")]
    EarlierOutcomeUnknown { operation: uuid::Uuid },
    #[error("an earlier send is still in flight")]
    DispatchBusy {
        operation: uuid::Uuid,
        pending: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum SessionStoreWaitError {
    #[error("the store didn't respond in time")]
    Elapsed,
    #[error("the session's store lane stopped")]
    Stopped,
    #[error("not sent: an earlier outcome is unknown")]
    EarlierOutcomeUnknown { operation: uuid::Uuid },
    #[error(transparent)]
    Store(#[from] ProviderOperationStoreError),
}

pub struct SessionStoreTicket {
    reply: oneshot::Receiver<Result<SessionStoreSettlement, ProviderOperationStoreError>>,
    deadline: ClockInstant,
    clock: Arc<dyn RuntimeClock>,
}

pub struct StoreLaneHandover {
    pub barrier: SessionStoreTicket,
    pub cleanups: Vec<(MarkedInput, SessionStoreTicket)>,
}

impl SessionStoreTicket {
    /// The budget includes lane backlog, store acquisition, write and commit.
    pub async fn wait(self) -> Result<SessionStoreSettlement, SessionStoreWaitError> {
        let clock = Arc::clone(&self.clock);
        router_clock::timeout_at(clock.as_ref(), self.deadline, self.settlement())
            .await
            .map_err(|_| SessionStoreWaitError::Elapsed)?
    }

    /// Executors can retain the exact offer correlation after a caller's L.
    pub async fn settlement(self) -> Result<SessionStoreSettlement, SessionStoreWaitError> {
        self.reply
            .await
            .map_err(|_| SessionStoreWaitError::Stopped)?
            .map_err(Into::into)
    }
}

#[derive(Clone)]
pub struct SessionStoreLane {
    inner: Arc<LaneQueue>,
}

pub struct SessionStoreLaneTask {
    termination: LaneTermination,
    writer: SessionStoreLaneWriter,
}

struct LaneTermination(SessionStoreLane);

struct SessionStoreLaneWriter {
    lane: SessionStoreLane,
    target: SessionRef,
    store: Arc<AsyncMutex<ProviderOperationStore>>,
}

struct LaneQueue {
    state: Mutex<LaneState>,
    target: SessionRef,
    binding: Arc<std::sync::OnceLock<Arc<BindingUncertainty>>>,
    ready: Notify,
    #[cfg(any(test, feature = "test-support"))]
    mark_accepted: Notify,
    clock: Arc<dyn RuntimeClock>,
}

struct LaneState {
    owner: OwnerIncarnation,
    closed: bool,
    queue: VecDeque<LaneEntry>,
    marks: BTreeMap<OperationId, AcceptedMark>,
    queued_admissions: BTreeMap<OperationId, QueuedAdmission>,
}

struct QueuedAdmission {
    owner: OwnerIncarnation,
    state: QueuedAdmissionState,
}

enum QueuedAdmissionState {
    Waiting(Box<ProviderOperationAdmission>),
    MarkOffered,
    MarkRefused,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum MarkStatus {
    Accepted,
    DefinitivelyFailed,
}

struct AcceptedMark {
    owner: OwnerIncarnation,
    input: InputId,
    status: MarkStatus,
    owner_input: Option<OwnerInput>,
    attempt: Option<uuid::Uuid>,
}

enum LaneRequest {
    Write(SessionStoreWrite),
    FirstQueuedDispatch {
        marked: MarkedInput,
        admission: Box<ProviderOperationAdmission>,
    },
    ReadSetup,
    SettleFailedOwnerMark(MarkedInput),
    Handover,
}

impl LaneRequest {
    fn written_operation(&self) -> Option<&OperationId> {
        match self {
            Self::Write(
                SessionStoreWrite::MarkDispatch(marked) | SessionStoreWrite::UndoDispatch(marked),
            )
            | Self::FirstQueuedDispatch { marked, .. }
            | Self::SettleFailedOwnerMark(marked) => Some(&marked.operation),
            Self::Write(
                SessionStoreWrite::Terminal { operation, .. }
                | SessionStoreWrite::ActivatedSession { operation, .. },
            ) => Some(operation),
            Self::Write(
                SessionStoreWrite::SaveMode(_)
                | SessionStoreWrite::ReconcileLateCancelledPrompt { .. },
            )
            | Self::ReadSetup
            | Self::Handover => None,
        }
    }
}

struct LaneEntry {
    owner: OwnerIncarnation,
    offered_at_ms: i64,
    request: LaneRequest,
    custody: Option<LaneWriteCustody>,
    dispatch: Option<LaneDispatchWrite>,
    reply: oneshot::Sender<Result<SessionStoreSettlement, ProviderOperationStoreError>>,
}

impl SessionStoreLane {
    pub fn new(
        owner: OwnerIncarnation,
        target: SessionRef,
        store: Arc<AsyncMutex<ProviderOperationStore>>,
        clock: Arc<dyn RuntimeClock>,
    ) -> (Self, SessionStoreLaneTask) {
        Self::with_binding(owner, target, store, clock, Arc::default())
    }

    pub(crate) fn with_binding(
        owner: OwnerIncarnation,
        target: SessionRef,
        store: Arc<AsyncMutex<ProviderOperationStore>>,
        clock: Arc<dyn RuntimeClock>,
        binding: Arc<std::sync::OnceLock<Arc<BindingUncertainty>>>,
    ) -> (Self, SessionStoreLaneTask) {
        let lane = Self {
            inner: Arc::new(LaneQueue {
                state: Mutex::new(LaneState {
                    owner,
                    closed: false,
                    queue: VecDeque::new(),
                    marks: BTreeMap::new(),
                    queued_admissions: BTreeMap::new(),
                }),
                target: target.clone(),
                binding,
                ready: Notify::new(),
                #[cfg(any(test, feature = "test-support"))]
                mark_accepted: Notify::new(),
                clock,
            }),
        };
        let task = SessionStoreLaneTask {
            termination: LaneTermination(lane.clone()),
            writer: SessionStoreLaneWriter {
                lane: lane.clone(),
                target,
                store,
            },
        };
        (lane, task)
    }

    fn state(&self) -> MutexGuard<'_, LaneState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn offer(
        &self,
        owner: OwnerIncarnation,
        write: SessionStoreWrite,
    ) -> Result<SessionStoreTicket, SessionStoreOfferError> {
        self.enqueue(owner, LaneRequest::Write(write), None)
    }

    pub fn read_setup(
        &self,
        owner: OwnerIncarnation,
    ) -> Result<SessionStoreTicket, SessionStoreOfferError> {
        self.enqueue(owner, LaneRequest::ReadSetup, None)
    }

    pub(crate) fn defer_queued_admission(
        &self,
        owner: OwnerIncarnation,
        admission: ProviderOperationAdmission,
    ) -> Result<(), SessionStoreOfferError> {
        let mut state = self.state();
        state.check(owner)?;
        state.queued_admissions.insert(
            admission.operation_id.clone(),
            QueuedAdmission {
                owner,
                state: QueuedAdmissionState::Waiting(Box::new(admission)),
            },
        );
        Ok(())
    }

    pub(crate) fn release_queued_admission(
        &self,
        owner: OwnerIncarnation,
        operation: &OperationId,
    ) {
        let mut state = self.state();
        if state
            .queued_admissions
            .get(operation)
            .is_some_and(|queued| {
                queued.owner == owner && matches!(queued.state, QueuedAdmissionState::Waiting(_))
            })
        {
            state.queued_admissions.remove(operation);
            state.marks.remove(operation);
        }
    }

    fn enqueue(
        &self,
        owner: OwnerIncarnation,
        request: LaneRequest,
        dispatch_offer: Option<LaneDispatchOffer>,
    ) -> Result<SessionStoreTicket, SessionStoreOfferError> {
        let (mut entry, ticket) = self.entry(owner, request);
        let mut state = self.state();
        state.check(owner)?;
        if let LaneRequest::Write(SessionStoreWrite::Terminal { operation, .. }) = &entry.request
            && state.has_no_queued_row(owner, operation)
        {
            state.queued_admissions.remove(operation);
            let _caller_gone = entry
                .reply
                .send(Ok(SessionStoreSettlement::MarkNeverWritten));
            return Ok(ticket);
        }
        let mut outcomes = self
            .inner
            .binding
            .get()
            .and_then(|binding| binding.outcomes.get())
            .map(lock);
        if let Some(offer) = &dispatch_offer
            && let Some(binding) = self.inner.binding.get()
        {
            let outcomes = outcomes
                .as_deref_mut()
                .ok_or(SessionStoreOfferError::Stopped)?;
            entry.dispatch =
                Some(self.accept_dispatch_write(owner, offer, &state, outcomes, binding)?);
        }
        if let LaneRequest::Write(SessionStoreWrite::MarkDispatch(marked)) = &entry.request {
            if self.inner.binding.get().is_some() && outcomes.is_none() {
                return Err(SessionStoreOfferError::Stopped);
            }
            if let Some(blocker) = outcomes.as_ref().and_then(|outcomes| {
                outcomes
                    .blockers(Some(&self.inner.target), true)
                    .into_iter()
                    .next()
            }) {
                let operation = uuid::Uuid::parse_str(blocker.as_str())
                    .map_err(|_| SessionStoreOfferError::Stopped)?;
                return Err(SessionStoreOfferError::EarlierOutcomeUnknown { operation });
            }
            state.marks.insert(
                marked.operation.clone(),
                AcceptedMark {
                    owner,
                    input: marked.input.clone(),
                    status: MarkStatus::Accepted,
                    owner_input: dispatch_offer.as_ref().map(|offer| offer.input.clone()),
                    attempt: entry.dispatch.as_ref().map(LaneDispatchWrite::attempt),
                },
            );
            #[cfg(any(test, feature = "test-support"))]
            self.inner.mark_accepted.notify_waiters();
            if let Some(QueuedAdmission {
                owner: waiting_owner,
                state: admission_state,
            }) = state.queued_admissions.get_mut(&marked.operation)
                && *waiting_owner == owner
                && matches!(admission_state, QueuedAdmissionState::Waiting(_))
                && let QueuedAdmissionState::Waiting(admission) =
                    std::mem::replace(admission_state, QueuedAdmissionState::MarkOffered)
            {
                entry.request = LaneRequest::FirstQueuedDispatch {
                    marked: marked.clone(),
                    admission,
                };
            }
        }
        if let LaneRequest::Write(SessionStoreWrite::Terminal {
            operation,
            effect: ProviderOperationEffect::Unknown,
            recovery,
            ..
        }) = &entry.request
        {
            if self.inner.binding.get().is_some() && outcomes.is_none() {
                return Err(SessionStoreOfferError::Stopped);
            }
            if let Some(outcomes) = &mut outcomes {
                outcomes.record_unknown(
                    operation.clone(),
                    Some(self.inner.target.clone()),
                    *recovery,
                );
                if let Some(binding) = self.inner.binding.get() {
                    binding.eligibility_changed();
                }
            }
        }
        entry.custody = self.borrow_write_custody(&entry.request);
        if let (Some(dispatch), Some(custody)) = (&entry.dispatch, &entry.custody) {
            custody.prepare_lane_projection(dispatch.attempt());
        }
        if let Some(custody) = &entry.custody
            && let LaneRequest::Write(SessionStoreWrite::Terminal {
                cancelled_input: Some(input),
                ..
            }) = &entry.request
        {
            custody.remember_cancelled_prompt(CancelledPromptIdentity {
                owner,
                input: (**input).clone(),
            });
        }
        state.queue.push_back(entry);
        drop(outcomes);
        self.inner.ready.notify_one();
        Ok(ticket)
    }

    /// The registry reads accepted marks after the failed task has stopped.
    /// No durable read is needed to classify an input that never took a slot.
    #[must_use]
    pub fn accepted_marks(&self, owner: OwnerIncarnation) -> Vec<MarkedInput> {
        self.state()
            .marks
            .iter()
            .filter(|(_, accepted)| accepted.owner == owner)
            .map(|(operation, accepted)| MarkedInput {
                operation: operation.clone(),
                input: accepted.input.clone(),
            })
            .collect()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn notice_fixture_wait(
        &self,
        wait: crate::binding_store_fixture::BindingFixtureWait,
    ) {
        if let Some(binding) = self.inner.binding.get() {
            binding.fixture.waiting(&self.inner.target, wait);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn wait_for_accepted_mark(&self, operation: &OperationId) {
        loop {
            let accepted = self.inner.mark_accepted.notified();
            tokio::pin!(accepted);
            accepted.as_mut().enable();
            if self.state().marks.contains_key(operation) {
                return;
            }
            accepted.await;
        }
    }

    pub fn handover(
        &self,
        previous: OwnerIncarnation,
        successor: OwnerIncarnation,
        known_unsent: Vec<MarkedInput>,
    ) -> Result<StoreLaneHandover, SessionStoreOfferError> {
        let mut state = self.state();
        state.check(previous)?;
        // Cleanup is enqueued by the registry's handover, after all accepted
        // old work and before the new owner's first offer. The marker result
        // may still be pending and is checked in the lane when cleanup runs.
        let mut cleanups = Vec::with_capacity(known_unsent.len());
        for marked in known_unsent {
            let (mut entry, ticket) =
                self.entry(previous, LaneRequest::SettleFailedOwnerMark(marked.clone()));
            entry.custody = self.borrow_write_custody(&entry.request);
            state.queue.push_back(entry);
            cleanups.push((marked, ticket));
        }
        let (barrier, ticket) = self.entry(previous, LaneRequest::Handover);
        state.queue.push_back(barrier);
        state.owner = successor;
        self.inner.ready.notify_one();
        Ok(StoreLaneHandover {
            barrier: ticket,
            cleanups,
        })
    }

    fn borrow_write_custody(&self, request: &LaneRequest) -> Option<LaneWriteCustody> {
        let operation = request.written_operation()?;
        let lookup = self.inner.binding.get()?.lane_custody.get()?;
        lookup(&self.inner.target, operation)
    }

    fn entry(
        &self,
        owner: OwnerIncarnation,
        request: LaneRequest,
    ) -> (LaneEntry, SessionStoreTicket) {
        let deadline = self.inner.clock.monotonic_now().after(STORE_WAIT);
        let offered_at_ms = self.inner.clock.utc_now().timestamp_millis();
        let (reply, receiver) = oneshot::channel();
        (
            LaneEntry {
                owner,
                offered_at_ms,
                request,
                custody: None,
                dispatch: None,
                reply,
            },
            SessionStoreTicket {
                reply: receiver,
                deadline,
                clock: Arc::clone(&self.inner.clock),
            },
        )
    }
}

impl LaneState {
    fn has_no_queued_row(&self, owner: OwnerIncarnation, operation: &OperationId) -> bool {
        self.queued_admissions.get(operation).is_some_and(|queued| {
            queued.owner == owner && !matches!(queued.state, QueuedAdmissionState::MarkOffered)
        })
    }

    fn check(&self, owner: OwnerIncarnation) -> Result<(), SessionStoreOfferError> {
        if self.closed {
            Err(SessionStoreOfferError::Stopped)
        } else if self.owner != owner {
            Err(SessionStoreOfferError::Replaced)
        } else {
            Ok(())
        }
    }
}

impl SessionStoreLaneTask {
    #[cfg(test)]
    pub async fn run(self, shutdown: CancellationToken) {
        self.run_tracked(shutdown, TaskTracker::new()).await;
    }

    pub(crate) async fn run_tracked(self, shutdown: CancellationToken, tasks: TaskTracker) {
        let _termination = self.termination;
        let writer = Arc::new(self.writer);
        let mut closing = false;
        loop {
            if !closing && shutdown.is_cancelled() {
                writer.lane.state().closed = true;
                closing = true;
            }
            if let Some(entry) = writer.take_entry() {
                // FIFO waits for the receipt. Stopping the runner cannot drop
                // a write after its transaction has submitted COMMIT.
                let writer = Arc::clone(&writer);
                if let Err(error) = tasks
                    .spawn(async move { writer.execute(entry).await })
                    .await
                {
                    tracing::error!(%error, "session store lane write task stopped");
                    return;
                }
            } else if closing {
                return;
            } else {
                tokio::select! {
                    () = shutdown.cancelled() => {}
                    () = writer.lane.inner.ready.notified() => {}
                }
            }
        }
    }
}

impl SessionStoreLaneWriter {
    fn take_entry(&self) -> Option<LaneEntry> {
        self.lane.state().queue.pop_front()
    }

    async fn execute(&self, entry: LaneEntry) {
        let LaneEntry {
            owner,
            offered_at_ms,
            request,
            custody,
            dispatch,
            reply,
        } = entry;
        let operation = request.written_operation().cloned();
        #[cfg(any(test, feature = "test-support"))]
        let late_repair = matches!(
            &request,
            LaneRequest::Write(SessionStoreWrite::ReconcileLateCancelledPrompt { .. })
        );
        let result = match request {
            LaneRequest::Write(write) => self.write(owner, write, offered_at_ms).await,
            LaneRequest::FirstQueuedDispatch { marked, admission } => {
                let result = self
                    .store
                    .lock()
                    .await
                    .admit_queued_prompt_at_dispatch(*admission, &self.target, offered_at_ms)
                    .await;
                self.record_mark_result(owner, &marked, &result);
                if matches!(result, Err(ProviderOperationStoreError::TransitionConflict))
                    && let Some(queued) = self
                        .lane
                        .state()
                        .queued_admissions
                        .get_mut(&marked.operation)
                    && queued.owner == owner
                {
                    queued.state = QueuedAdmissionState::MarkRefused;
                }
                result.map(|row| SessionStoreSettlement::Operation(Box::new(row)))
            }
            LaneRequest::ReadSetup => self
                .store
                .lock()
                .await
                .session_record(&self.target)
                .await
                .map(|setup| SessionStoreSettlement::Setup(setup.map(Box::new))),
            LaneRequest::Handover => {
                let mut state = self.lane.state();
                state.marks.retain(|_, mark| mark.owner != owner);
                state
                    .queued_admissions
                    .retain(|_, queued| queued.owner != owner);
                Ok(SessionStoreSettlement::HandedOver)
            }
            LaneRequest::SettleFailedOwnerMark(marked) => {
                let needs_cleanup =
                    self.lane
                        .state()
                        .marks
                        .get(&marked.operation)
                        .is_some_and(|mark| {
                            mark.owner == owner
                                && mark.input == marked.input
                                && mark.status != MarkStatus::DefinitivelyFailed
                        });
                if needs_cleanup {
                    self.write(
                        owner,
                        SessionStoreWrite::Terminal {
                            operation: marked.operation,
                            effect: ProviderOperationEffect::None,
                            reconciliation: ProviderReconciliationState::Confirmed,
                            stop_reason: None,
                            recovery: UnknownRecovery::Blocks,
                            cancelled_input: None,
                        },
                        offered_at_ms,
                    )
                    .await
                } else {
                    Ok(SessionStoreSettlement::MarkNeverWritten)
                }
            }
        };
        if result.is_err()
            && let (Some(binding), Some(operation), Some(_)) =
                (self.lane.inner.binding.get(), operation, &custody)
        {
            let observed = self.store.lock().await.inspect(&operation).await;
            if observed.as_ref().is_err()
                || observed
                    .as_ref()
                    .ok()
                    .and_then(Option::as_ref)
                    .is_some_and(|row| row.target.as_ref() == Some(&self.target))
            {
                binding.preserve_residual(&operation, Some(self.target.clone()), observed);
            }
        }
        if let Some(dispatch) = dispatch {
            self.project_dispatch_receipt(dispatch, custody.as_ref(), &result)
                .await;
            #[cfg(any(test, feature = "test-support"))]
            if let Ok(SessionStoreSettlement::Operation(row)) = &result
                && row.stage == ProviderOperationStage::MayHaveDispatched
            {
                self.lane.notice_fixture_wait(
                    crate::binding_store_fixture::BindingFixtureWait::QueuedDispatchObserved(
                        row.operation_id.clone(),
                    ),
                );
            }
        }
        if let Some(custody) = custody {
            custody.settle(match &result {
                Ok(SessionStoreSettlement::MarkNeverWritten) => CustodyEnd::NoWrite,
                Ok(_) => CustodyEnd::Written,
                Err(_) => CustodyEnd::Failed,
            });
        }
        #[cfg(any(test, feature = "test-support"))]
        if late_repair && let Some(binding) = self.lane.inner.binding.get() {
            binding.fixture.waiting(
                &self.target,
                crate::binding_store_fixture::BindingFixtureWait::LatePromptRepair,
            );
        }
        let _caller_stopped_waiting = reply.send(result);
    }

    async fn write(
        &self,
        owner: OwnerIncarnation,
        request: SessionStoreWrite,
        offered_at_ms: i64,
    ) -> Result<SessionStoreSettlement, ProviderOperationStoreError> {
        if let SessionStoreWrite::Terminal { operation, .. } = &request {
            let mut state = self.lane.state();
            if state.has_no_queued_row(owner, operation) {
                state.queued_admissions.remove(operation);
                state.marks.remove(operation);
                return Ok(SessionStoreSettlement::MarkNeverWritten);
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        if let Some(binding) = self.lane.inner.binding.get() {
            match &request {
                SessionStoreWrite::Terminal { operation, .. } => {
                    binding.fixture.hold_terminal_write(operation).await
                }
                SessionStoreWrite::SaveMode(value) => {
                    binding.fixture.hold_mode_write(&self.target, value).await
                }
                _ => {}
            }
        }
        let mut store = self.store.lock().await;
        match request {
            SessionStoreWrite::MarkDispatch(marked) => {
                let result = store
                    .mark_may_have_dispatched(&marked.operation, offered_at_ms)
                    .await;
                self.record_mark_result(owner, &marked, &result);
                result.map(|row| SessionStoreSettlement::Operation(Box::new(row)))
            }
            SessionStoreWrite::UndoDispatch(marked) => {
                let row = store
                    .unmark_undispatched(&marked.operation, offered_at_ms)
                    .await?;
                self.lane.state().marks.remove(&marked.operation);
                Ok(SessionStoreSettlement::Operation(Box::new(row)))
            }
            SessionStoreWrite::ReconcileLateCancelledPrompt { operation, input } => {
                self.repair_cancelled_prompt(
                    &mut store,
                    operation,
                    CancelledPromptIdentity {
                        owner,
                        input: *input,
                    },
                    offered_at_ms,
                )
                .await
            }
            SessionStoreWrite::Terminal {
                operation,
                effect,
                reconciliation,
                stop_reason,
                ..
            } => {
                let row = store
                    .record_terminal(
                        &operation,
                        effect,
                        reconciliation,
                        stop_reason,
                        offered_at_ms,
                    )
                    .await?;
                let mut state = self.lane.state();
                state.marks.remove(&operation);
                state.queued_admissions.remove(&operation);
                Ok(SessionStoreSettlement::Operation(Box::new(row)))
            }
            SessionStoreWrite::ActivatedSession { operation, record } => {
                store.settle_session_operation(&operation, &record).await?;
                Ok(SessionStoreSettlement::Activated)
            }
            SessionStoreWrite::SaveMode(requested) => {
                store
                    .record_session_mode(&self.target, &requested, offered_at_ms)
                    .await?;
                Ok(SessionStoreSettlement::ModeSaved)
            }
        }
    }

    async fn repair_cancelled_prompt(
        &self,
        store: &mut ProviderOperationStore,
        operation: OperationId,
        identity: CancelledPromptIdentity,
        offered_at_ms: i64,
    ) -> Result<SessionStoreSettlement, ProviderOperationStoreError> {
        // A queued repair is only evidence. Authority starts under the store
        // guard and requires the original still-retained lifecycle.
        let binding = self
            .lane
            .inner
            .binding
            .get()
            .ok_or(ProviderOperationStoreError::TransitionConflict)?;
        let authority = binding
            .late_prompt_repair
            .get()
            .and_then(|authorize| authorize(&self.target, &operation, &identity))
            .ok_or(ProviderOperationStoreError::TransitionConflict)?;
        let row = store
            .inspect(&operation)
            .await?
            .ok_or(ProviderOperationStoreError::NotFound)?;
        if row.target.as_ref() != Some(&self.target)
            || row.operation_kind != ProviderOperationKind::ConversationPrompt
            || row.stage != ProviderOperationStage::Terminal
            || row.effect != ProviderOperationEffect::Unknown
            || !matches!(
                row.reconciliation_state,
                ProviderReconciliationState::Unresolved
                    | ProviderReconciliationState::NotReconcilable
            )
            || !matches!(&row.binding, ConversationBindingIdentity::ExternalProvider { binding }
                if binding.endpoint == self.target.endpoint && binding.generation == authority.generation)
        {
            return Err(ProviderOperationStoreError::TransitionConflict);
        }
        let row = store
            .confirm_recorded_target(&operation, offered_at_ms)
            .await?;
        if let Some(outcomes) = binding.outcomes.get() {
            lock(outcomes).record_reconciled(operation);
        }
        binding.eligibility_changed();
        Ok(SessionStoreSettlement::Operation(Box::new(row)))
    }

    fn record_mark_result(
        &self,
        owner: OwnerIncarnation,
        marked: &MarkedInput,
        result: &Result<ProviderOperationRecord, ProviderOperationStoreError>,
    ) {
        if matches!(
            result,
            Err(ProviderOperationStoreError::NotFound
                | ProviderOperationStoreError::TransitionConflict)
        ) && let Some(mark) = self.lane.state().marks.get_mut(&marked.operation)
            && mark.owner == owner
            && mark.input == marked.input
        {
            mark.status = MarkStatus::DefinitivelyFailed;
        }
    }
}

impl Drop for LaneTermination {
    fn drop(&mut self) {
        let queued = {
            let mut state = self.0.state();
            state.closed = true;
            std::mem::take(&mut state.queue)
        };
        // These entries never reached the writer. Release their exact tickets
        // outside the queue lock and let each reply channel close.
        for entry in queued {
            if let Some(custody) = entry.custody {
                custody.settle(CustodyEnd::Released);
            }
        }
    }
}

#[cfg(test)]
#[path = "session_store_lane_test.rs"]
pub(crate) mod tests;
