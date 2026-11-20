//! Ordered operation and saved-setup stores for a single provider session.

use collaboration_protocol::{
    OperationId, ProviderOperationEffect, ProviderPromptStopReason, ProviderReconciliationState,
    SessionRef,
};
use collaboration_runtime::{
    ProviderOperationRecord, ProviderOperationStore, ProviderOperationStoreError,
    ProviderSessionRecord,
};
use provider_session_machines::session_owner::OwnerIncarnation;
use router_clock::{ClockInstant, RuntimeClock};
use session_event_model::InputId;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, Notify, oneshot};
use tokio_util::sync::CancellationToken;

const STORE_WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkedInput {
    pub input: InputId,
    pub operation: OperationId,
}

#[derive(Debug)]
pub enum SessionStoreWrite {
    MarkDispatch(MarkedInput),
    UndoDispatch(MarkedInput),
    Terminal {
        operation: OperationId,
        effect: ProviderOperationEffect,
        reconciliation: ProviderReconciliationState,
        stop_reason: Option<ProviderPromptStopReason>,
    },
    SaveMode(String),
}

#[derive(Debug)]
pub enum SessionStoreSettlement {
    Operation(Box<ProviderOperationRecord>),
    Setup(Option<Box<ProviderSessionRecord>>),
    ModeSaved,
    MarkNeverWritten,
    HandedOver,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SessionStoreOfferError {
    #[error("this session has a different current owner")]
    Replaced,
    #[error("the session's store lane stopped")]
    Stopped,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionStoreWaitError {
    #[error("the store didn't respond in time")]
    Elapsed,
    #[error("the session's store lane stopped")]
    Stopped,
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
    lane: SessionStoreLane,
    target: SessionRef,
    store: Arc<AsyncMutex<ProviderOperationStore>>,
}

struct LaneQueue {
    state: Mutex<LaneState>,
    ready: Notify,
    clock: Arc<dyn RuntimeClock>,
}

struct LaneState {
    owner: OwnerIncarnation,
    closed: bool,
    queue: VecDeque<LaneEntry>,
    marks: BTreeMap<OperationId, AcceptedMark>,
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
}

enum LaneRequest {
    Write(SessionStoreWrite),
    ReadSetup,
    SettleFailedOwnerMark(MarkedInput),
    Handover,
}

struct LaneEntry {
    owner: OwnerIncarnation,
    offered_at_ms: i64,
    request: LaneRequest,
    reply: oneshot::Sender<Result<SessionStoreSettlement, ProviderOperationStoreError>>,
}

impl SessionStoreLane {
    pub fn new(
        owner: OwnerIncarnation,
        target: SessionRef,
        store: Arc<AsyncMutex<ProviderOperationStore>>,
        clock: Arc<dyn RuntimeClock>,
    ) -> (Self, SessionStoreLaneTask) {
        let lane = Self {
            inner: Arc::new(LaneQueue {
                state: Mutex::new(LaneState {
                    owner,
                    closed: false,
                    queue: VecDeque::new(),
                    marks: BTreeMap::new(),
                }),
                ready: Notify::new(),
                clock,
            }),
        };
        let task = SessionStoreLaneTask {
            lane: lane.clone(),
            target,
            store,
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
        self.enqueue(owner, LaneRequest::Write(write))
    }

    pub fn read_setup(
        &self,
        owner: OwnerIncarnation,
    ) -> Result<SessionStoreTicket, SessionStoreOfferError> {
        self.enqueue(owner, LaneRequest::ReadSetup)
    }

    fn enqueue(
        &self,
        owner: OwnerIncarnation,
        request: LaneRequest,
    ) -> Result<SessionStoreTicket, SessionStoreOfferError> {
        let (entry, ticket) = self.entry(owner, request);
        let mut state = self.state();
        state.check(owner)?;
        if let LaneRequest::Write(SessionStoreWrite::MarkDispatch(marked)) = &entry.request {
            state.marks.insert(
                marked.operation.clone(),
                AcceptedMark {
                    owner,
                    input: marked.input.clone(),
                    status: MarkStatus::Accepted,
                },
            );
        }
        state.queue.push_back(entry);
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
            let (entry, ticket) =
                self.entry(previous, LaneRequest::SettleFailedOwnerMark(marked.clone()));
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
    pub async fn run(self, shutdown: CancellationToken) {
        let mut closing = false;
        loop {
            if !closing && shutdown.is_cancelled() {
                self.lane.state().closed = true;
                closing = true;
            }
            if let Some(entry) = self.take_entry() {
                self.execute(entry).await;
            } else if closing {
                return;
            } else {
                tokio::select! {
                    () = shutdown.cancelled() => {}
                    () = self.lane.inner.ready.notified() => {}
                }
            }
        }
    }

    fn take_entry(&self) -> Option<LaneEntry> {
        self.lane.state().queue.pop_front()
    }

    async fn execute(&self, entry: LaneEntry) {
        let LaneEntry {
            owner,
            offered_at_ms,
            request,
            reply,
        } = entry;
        let result = match request {
            LaneRequest::Write(write) => self.write(owner, write, offered_at_ms).await,
            LaneRequest::ReadSetup => self
                .store
                .lock()
                .await
                .session_record(&self.target)
                .await
                .map(|setup| SessionStoreSettlement::Setup(setup.map(Box::new))),
            LaneRequest::Handover => {
                self.lane
                    .state()
                    .marks
                    .retain(|_, mark| mark.owner != owner);
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
                        },
                        offered_at_ms,
                    )
                    .await
                } else {
                    Ok(SessionStoreSettlement::MarkNeverWritten)
                }
            }
        };
        let _caller_stopped_waiting = reply.send(result);
    }

    async fn write(
        &self,
        owner: OwnerIncarnation,
        request: SessionStoreWrite,
        offered_at_ms: i64,
    ) -> Result<SessionStoreSettlement, ProviderOperationStoreError> {
        let mut store = self.store.lock().await;
        match request {
            SessionStoreWrite::MarkDispatch(marked) => {
                let result = store
                    .mark_may_have_dispatched(&marked.operation, offered_at_ms)
                    .await;
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
                result.map(|row| SessionStoreSettlement::Operation(Box::new(row)))
            }
            SessionStoreWrite::UndoDispatch(marked) => {
                let row = store
                    .unmark_undispatched(&marked.operation, offered_at_ms)
                    .await?;
                self.lane.state().marks.remove(&marked.operation);
                Ok(SessionStoreSettlement::Operation(Box::new(row)))
            }
            SessionStoreWrite::Terminal {
                operation,
                effect,
                reconciliation,
                stop_reason,
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
                self.lane.state().marks.remove(&operation);
                Ok(SessionStoreSettlement::Operation(Box::new(row)))
            }
            SessionStoreWrite::SaveMode(requested) => {
                store
                    .record_session_mode(&self.target, &requested, offered_at_ms)
                    .await?;
                Ok(SessionStoreSettlement::ModeSaved)
            }
        }
    }
}

impl Drop for SessionStoreLaneTask {
    fn drop(&mut self) {
        let mut state = self.lane.state();
        state.closed = true;
        state.queue.clear();
    }
}

#[cfg(test)]
#[path = "session_store_lane_test.rs"]
pub(crate) mod tests;
