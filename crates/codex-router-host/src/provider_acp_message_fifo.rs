//! Bounded, Host-lifetime FIFO for provider prompts accepted as queued.
use crate::{
    ExternalProviderSupervisor, LiveSessionOwnershipCheck,
    external_provider_supervisor::ProviderPromptContentsRequest,
    external_provider_supervisor::ProviderPromptDispatch,
    provider_acp_session_loading::{ProviderSessionLoadOutcome, ensure_provider_session_loaded},
};
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::NOT_LOADED_REASON;
use collaboration_contracts::ProviderConversationBackend;
use collaboration_protocol::{
    ConversationOperationFailureKind, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, PositiveSeconds, ProviderOperationStage, SessionRef,
};
use collaboration_runtime::ProviderOperationStore;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const QUEUE_CAPACITY: usize = 128;
const MAX_SESSION_QUEUES: usize = 1024;
const SETTLEMENT_WAIT_SECONDS: u32 = 10;
const MIN_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);
const MAX_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);
const PROVIDER_RETIRED_REASON: &str = "providerRetired";

#[derive(Clone)]
pub(crate) enum ProviderQueuedPrompt {
    Contents {
        request: ProviderPromptContentsRequest,
        load_policy: LoadPolicy,
        /// An API prompt admitted before it was queued (D1 R12); mail and
        /// content-command entries are admitted when dispatched.
        admitted: Option<crate::external_provider_supervisor::AdmittedApiPrompt>,
    },
}

impl ProviderQueuedPrompt {
    fn operation_id(&self) -> &collaboration_protocol::OperationId {
        match self {
            Self::Contents { request, .. } => &request.operation_id,
        }
    }

    fn load_policy(&self) -> LoadPolicy {
        match self {
            Self::Contents { load_policy, .. } => *load_policy,
        }
    }
}

/// One session's line: its channel and the state its worker shares.
struct SessionLine {
    sender: mpsc::Sender<QueuedEntry>,
    state: Arc<SessionLineState>,
}

/// An entry in a session's line, numbered when it was sent into the line.
struct QueuedEntry {
    sequence: u64,
    prompt: ProviderQueuedPrompt,
}

/// Room for one entry in a session's line; sending numbers the entry.
pub(crate) struct QueuePermit {
    permit: mpsc::OwnedPermit<QueuedEntry>,
    line: Arc<SessionLineState>,
}

impl QueuePermit {
    pub(crate) fn send(self, prompt: ProviderQueuedPrompt) {
        let sequence = self
            .line
            .sent
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.permit.send(QueuedEntry { sequence, prompt });
    }
}

/// What a session's worker and its line share.
struct SessionLineState {
    /// The worker holds an entry it took from the channel (D1 R12: the 128
    /// waiting entries count the held head).
    holding: std::sync::atomic::AtomicBool,
    /// Stops this session's worker at Router shutdown.
    stop: CancellationToken,
    /// Entries sent into the line so far.
    sent: std::sync::atomic::AtomicU64,
    /// Entries numbered below this were in the line when the session
    /// closed: none of them is loaded or prompted (D1 R14).
    closed_before: std::sync::atomic::AtomicU64,
    /// Why the session closed, when it did.
    close_reason: StdMutex<Option<String>>,
    /// Wakes a worker waiting with its held head when the session closes.
    closed: tokio::sync::Notify,
}

impl SessionLineState {
    fn new(stop: CancellationToken) -> Self {
        Self {
            holding: std::sync::atomic::AtomicBool::new(false),
            stop,
            sent: std::sync::atomic::AtomicU64::new(0),
            closed_before: std::sync::atomic::AtomicU64::new(0),
            close_reason: StdMutex::new(None),
            closed: tokio::sync::Notify::new(),
        }
    }

    /// The close reason, when this entry was in the line as its session closed.
    fn closed_reason_for(&self, sequence: u64) -> Option<String> {
        if sequence < self.closed_before.load(std::sync::atomic::Ordering::SeqCst) {
            self.close_reason
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        } else {
            None
        }
    }
}

pub(crate) struct ProviderAcpMessageFifo {
    #[cfg(test)]
    wait_started: Arc<tokio::sync::Notify>,
    supervisor: Arc<ExternalProviderSupervisor>,
    store: Arc<Mutex<ProviderOperationStore>>,
    ownership: Arc<dyn LiveSessionOwnershipCheck>,
    senders: StdMutex<HashMap<SessionRef, SessionLine>>,
    /// The admitted API entries in a line, so a queue cancel settles its
    /// waiter at once (D1 R14). Dropped once dispatched or settled.
    admitted_waiting: StdMutex<
        HashMap<
            collaboration_protocol::OperationId,
            crate::external_provider_supervisor::AdmittedApiPrompt,
        >,
    >,
    workers: StdMutex<Vec<JoinHandle<()>>>,
    shutdown: CancellationToken,
}

impl ProviderAcpMessageFifo {
    pub(crate) fn new(
        supervisor: Arc<ExternalProviderSupervisor>,
        store: Arc<Mutex<ProviderOperationStore>>,
        ownership: Arc<dyn LiveSessionOwnershipCheck>,
    ) -> Self {
        Self {
            #[cfg(test)]
            wait_started: Arc::new(tokio::sync::Notify::new()),
            supervisor,
            store,
            ownership,
            senders: StdMutex::new(HashMap::new()),
            admitted_waiting: StdMutex::new(HashMap::new()),
            workers: StdMutex::new(Vec::new()),
            shutdown: CancellationToken::new(),
        }
    }

    pub(crate) fn reserve(&self, target: &SessionRef) -> Result<QueuePermit, &'static str> {
        if self.shutdown.is_cancelled() {
            return Err("provider queue is shutting down");
        }
        let mut senders = self
            .senders
            .lock()
            .map_err(|_| "provider queue unavailable")?;
        let (sender, line) =
            if let Some(line) = senders.get(target).filter(|line| !line.sender.is_closed()) {
                let waiting = QUEUE_CAPACITY.saturating_sub(line.sender.capacity())
                    + usize::from(line.state.holding.load(std::sync::atomic::Ordering::SeqCst));
                if waiting >= QUEUE_CAPACITY {
                    return Err("provider session queue is full");
                }
                (line.sender.clone(), Arc::clone(&line.state))
            } else {
                if senders.len() >= MAX_SESSION_QUEUES {
                    return Err("provider session queue capacity exceeded");
                }
                let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
                let state = Arc::new(SessionLineState::new(self.shutdown.child_token()));
                let worker = tokio::spawn(run_provider_message_fifo(
                    target.clone(),
                    receiver,
                    Arc::clone(&self.supervisor),
                    Arc::clone(&self.store),
                    Arc::clone(&self.ownership),
                    Arc::clone(&state),
                    #[cfg(test)]
                    Arc::clone(&self.wait_started),
                ));
                self.workers
                    .lock()
                    .map_err(|_| "provider queue workers unavailable")?
                    .push(worker);
                senders.insert(
                    target.clone(),
                    SessionLine {
                        sender: sender.clone(),
                        state: Arc::clone(&state),
                    },
                );
                (sender, state)
            };
        let permit = sender
            .try_reserve_owned()
            .map_err(|_| "provider session queue is full")?;
        Ok(QueuePermit { permit, line })
    }

    /// Queues an admitted API prompt; refused when the session's line is full.
    pub(crate) fn enqueue_admitted_api_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        binding: collaboration_protocol::ProviderBindingIdentity,
        admitted: crate::external_provider_supervisor::AdmittedApiPrompt,
    ) -> Result<crate::ProviderQueuedInput, &'static str> {
        let permit = self.reserve(&request.target)?;
        let contents = match &request.input {
            acp_client_runtime::ProviderTurnInput::Prompt(submission) => submission.content.clone(),
            acp_client_runtime::ProviderTurnInput::Compact => Vec::new(),
        };
        let queued = self
            .supervisor
            .queued_operation_registry()
            .record_queued_contents(
                request.operation_id.clone(),
                request.target.clone(),
                binding,
                request.input_id.clone(),
                &contents,
            );
        {
            let mut waiting = self
                .admitted_waiting
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            waiting.retain(|_, entry| !entry.is_settled());
            waiting.insert(request.operation_id.clone(), admitted.clone());
        }
        permit.send(ProviderQueuedPrompt::Contents {
            request,
            load_policy: LoadPolicy::MayLoad,
            admitted: Some(admitted),
        });
        Ok(queued)
    }

    /// Cancels a waiting entry. An admitted API prompt ends not sent at once,
    /// through its one completion owner, so its waiter wakes now rather than
    /// when the worker reaches it.
    pub(crate) fn cancel_queued(
        &self,
        target: &SessionRef,
        input_id: &session_event_model::InputId,
    ) -> Result<(), crate::ProviderQueueCancellationError> {
        let operation_id = self
            .supervisor
            .queued_operation_registry()
            .cancel(target, input_id)?;
        let admitted = self
            .admitted_waiting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&operation_id);
        if let Some(admitted) = admitted {
            self.supervisor.complete_unsent_api_entry(
                &admitted,
                &operation_id,
                target,
                "cancelled from the queue",
            );
        }
        Ok(())
    }

    /// The session closed: every entry not yet dispatched, the held head
    /// included, ends not sent with the reason, and its worker stops (D1 R14).
    /// An entry already dispatched keeps its real outcome.
    pub(crate) fn close_session(&self, target: &SessionRef, reason: &str) {
        let line = self
            .senders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(target)
            .map(|line| Arc::clone(&line.state));
        if let Some(line) = line {
            *line
                .close_reason
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(reason.to_owned());
            line.closed_before.store(
                line.sent.load(std::sync::atomic::Ordering::SeqCst),
                std::sync::atomic::Ordering::SeqCst,
            );
            line.closed.notify_waiters();
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.shutdown.cancel();
        let workers = match self.workers.lock() {
            Ok(mut workers) => std::mem::take(&mut *workers),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };
        for worker in workers {
            let _result = worker.await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_workers(&self) {
        let workers = match self.workers.lock() {
            Ok(mut workers) => std::mem::take(&mut *workers),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };
        for worker in workers {
            worker.await.expect("provider queue worker completes");
        }
    }
}

impl Drop for ProviderAcpMessageFifo {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Ok(mut workers) = self.workers.lock() {
            for worker in workers.drain(..) {
                worker.abort();
            }
        }
    }
}

async fn run_provider_message_fifo(
    target: SessionRef,
    mut receiver: mpsc::Receiver<QueuedEntry>,
    supervisor: Arc<ExternalProviderSupervisor>,
    store: Arc<Mutex<ProviderOperationStore>>,
    ownership: Arc<dyn LiveSessionOwnershipCheck>,
    line: Arc<SessionLineState>,
    #[cfg(test)] wait_started: Arc<tokio::sync::Notify>,
) {
    let shutdown = line.stop.clone();
    loop {
        line.holding
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let request = tokio::select! {
            () = shutdown.cancelled() => {
                mark_remaining_not_submitted(&supervisor, &mut receiver, "Router shutdown before provider submission");
                return;
            },
            entry = receiver.recv() => match entry {
                Some(entry) => entry,
                None => return,
            },
        };
        line.holding
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let QueuedEntry {
            sequence,
            prompt: request,
        } = request;
        if let Some(reason) = line.closed_reason_for(sequence) {
            mark_entry_not_sent(&supervisor, &request, reason);
            continue;
        }
        let operation_id = request.operation_id().clone();
        let load_policy = request.load_policy();
        let Some(runtime) = supervisor.runtime_for(&target.endpoint) else {
            drop_current_and_remaining(
                &supervisor,
                &request,
                &mut receiver,
                PROVIDER_RETIRED_REASON,
            );
            return;
        };
        let retirement = runtime.retirement();
        let mut retry_delay = MIN_RETRY_DELAY;
        let mut loaded = false;
        let mut operation_admitted = false;
        let mut dispatch_started = false;
        loop {
            if let Some(reason) = line.closed_reason_for(sequence) {
                mark_entry_not_sent(&supervisor, &request, reason);
                break;
            }
            if shutdown.is_cancelled() {
                drop_current_and_remaining(
                    &supervisor,
                    &request,
                    &mut receiver,
                    "Router shutdown before provider submission",
                );
                return;
            }
            if retirement.is_cancelled() {
                drop_current_and_remaining(
                    &supervisor,
                    &request,
                    &mut receiver,
                    PROVIDER_RETIRED_REASON,
                );
                return;
            }
            if !loaded {
                let load_outcome = tokio::select! {
                    () = shutdown.cancelled() => {
                        drop_current_and_remaining(&supervisor, &request, &mut receiver, "Router shutdown before provider submission");
                        return;
                    },
                    () = retirement.cancelled() => {
                        drop_current_and_remaining(&supervisor, &request, &mut receiver, PROVIDER_RETIRED_REASON);
                        return;
                    },
                    outcome = ensure_provider_session_loaded(&supervisor, &store, ownership.as_ref(), &target, load_policy) => outcome,
                };
                match load_outcome {
                    ProviderSessionLoadOutcome::Ready
                    | ProviderSessionLoadOutcome::AlreadyLoaded => {
                        loaded = true;
                    }
                    ProviderSessionLoadOutcome::NotLoaded => {
                        mark_entry_not_sent(&supervisor, &request, NOT_LOADED_REASON);
                        break;
                    }
                    ProviderSessionLoadOutcome::UnsupportedLoad => {
                        mark_entry_not_sent(&supervisor, &request, "unsupported: load");
                        break;
                    }
                    ProviderSessionLoadOutcome::MissingRecord => {
                        mark_entry_not_sent(
                            &supervisor,
                            &request,
                            "provider session record is missing",
                        );
                        break;
                    }
                    ProviderSessionLoadOutcome::LiveElsewhere => {
                        mark_entry_not_sent(&supervisor, &request, "session is live elsewhere");
                        break;
                    }
                    ProviderSessionLoadOutcome::Unavailable { reason } => {
                        mark_entry_not_sent(&supervisor, &request, reason);
                        break;
                    }
                    ProviderSessionLoadOutcome::Rejected { reason } => {
                        mark_entry_not_sent(&supervisor, &request, reason.safe_detail());
                        break;
                    }
                }
            }
            #[cfg(test)]
            wait_started.notify_one();
            let idle = tokio::select! {
                () = shutdown.cancelled() => {
                    drop_current_and_remaining(&supervisor, &request, &mut receiver, "Router shutdown before provider submission");
                    return;
                },
                () = retirement.cancelled() => {
                    drop_current_and_remaining(&supervisor, &request, &mut receiver, PROVIDER_RETIRED_REASON);
                    return;
                },
                () = line.closed.notified() => continue,
                idle = runtime.wait_session_idle(String::from(target.session_id.clone())) => idle,
            };
            if idle.is_err() {
                if !wait_before_retry(&shutdown, &retirement, retry_delay).await {
                    drop_current_and_remaining(
                        &supervisor,
                        &request,
                        &mut receiver,
                        interrupted_queue_reason(&shutdown, &retirement),
                    );
                    return;
                }
                retry_delay = next_retry_delay(retry_delay);
                continue;
            }
            if !dispatch_started {
                if !supervisor
                    .queued_operation_registry()
                    .try_start(&operation_id)
                {
                    break;
                }
                dispatch_started = true;
            }
            let submitted = tokio::select! {
                () = shutdown.cancelled() => {
                    drop_current_and_remaining(&supervisor, &request, &mut receiver, "Router shutdown before provider submission");
                    return;
                },
                () = retirement.cancelled() => {
                    drop_current_and_remaining(&supervisor, &request, &mut receiver, PROVIDER_RETIRED_REASON);
                    return;
                },
                submitted = async {
                    match request.clone() {
                        ProviderQueuedPrompt::Contents { request, admitted: Some(admitted), .. } =>
                            supervisor.dispatch_queued_api_prompt(request, &admitted).await,
                        ProviderQueuedPrompt::Contents { request, admitted: None, .. } =>
                            supervisor.submit_delivery_prompt_contents(request).await,
                    }
                } => submitted,
            };
            match submitted {
                Ok(
                    ProviderPromptDispatch::Submitted
                    | ProviderPromptDispatch::NotSubmitted
                    | ProviderPromptDispatch::Refused(_)
                    | ProviderPromptDispatch::Existing
                    | ProviderPromptDispatch::Uncertain,
                ) => {
                    // This ID has been admitted. From here on, observe this exact
                    // operation; never submit its logical message under the same ID again.
                    operation_admitted = true;
                    break;
                }
                Err(failure) if failure.kind == ConversationOperationFailureKind::Busy => {}
                Err(failure) => {
                    mark_entry_not_sent(&supervisor, &request, String::from(failure.message));
                    break;
                }
            }
            if !wait_before_retry(&shutdown, &retirement, retry_delay).await {
                drop_current_and_remaining(
                    &supervisor,
                    &request,
                    &mut receiver,
                    interrupted_queue_reason(&shutdown, &retirement),
                );
                return;
            }
            retry_delay = next_retry_delay(retry_delay);
        }
        if !operation_admitted {
            continue;
        }
        let Ok(wait_seconds) = PositiveSeconds::try_from(SETTLEMENT_WAIT_SECONDS) else {
            continue;
        };
        loop {
            let settled = tokio::select! {
                () = shutdown.cancelled() => {
                    mark_remaining_not_submitted(&supervisor, &mut receiver, "Router shutdown before queued prompt settlement");
                    return;
                },
                () = retirement.cancelled() => {
                    mark_remaining_not_submitted(&supervisor, &mut receiver, PROVIDER_RETIRED_REASON);
                    return;
                },
                result = supervisor.wait(ConversationOperationWaitRequest {
                    operation_id: operation_id.clone(), timeout_seconds: wait_seconds,
                }) => result,
            };
            match settled {
                Ok(result)
                    if result.operation.stage == ProviderOperationStage::Terminal
                        && !matches!(result.output, ConversationOperationWaitOutput::Pending) =>
                {
                    supervisor.queued_operation_registry().clear(&operation_id);
                    break;
                }
                Ok(_) => {}
                Err(_failure) => {
                    let terminal_record = store.lock().await.inspect(&operation_id).await;
                    if let Ok(Some(record)) = terminal_record
                        && record.stage == ProviderOperationStage::Terminal
                    {
                        supervisor.queued_operation_registry().clear(&operation_id);
                        break;
                    }
                    if !wait_before_retry(&shutdown, &retirement, MIN_RETRY_DELAY).await {
                        mark_remaining_not_submitted(
                            &supervisor,
                            &mut receiver,
                            interrupted_queue_reason(&shutdown, &retirement),
                        );
                        return;
                    }
                }
            }
        }
    }
}

fn interrupted_queue_reason(
    shutdown: &CancellationToken,
    retirement: &CancellationToken,
) -> &'static str {
    if retirement.is_cancelled() {
        PROVIDER_RETIRED_REASON
    } else if shutdown.is_cancelled() {
        "Router shutdown before queued prompt submission"
    } else {
        "provider queue stopped before queued prompt submission"
    }
}

async fn wait_before_retry(
    shutdown: &CancellationToken,
    retirement: &CancellationToken,
    delay: std::time::Duration,
) -> bool {
    tokio::select! {
        () = shutdown.cancelled() => false,
        () = retirement.cancelled() => false,
        () = tokio::time::sleep(delay) => true,
    }
}

fn next_retry_delay(current: std::time::Duration) -> std::time::Duration {
    current.saturating_mul(2).min(MAX_RETRY_DELAY)
}

fn drop_current_and_remaining(
    supervisor: &ExternalProviderSupervisor,
    current: &ProviderQueuedPrompt,
    receiver: &mut mpsc::Receiver<QueuedEntry>,
    reason: &str,
) {
    mark_entry_not_sent(supervisor, current, reason);
    mark_remaining_not_submitted(supervisor, receiver, reason);
}

fn mark_remaining_not_submitted(
    supervisor: &ExternalProviderSupervisor,
    receiver: &mut mpsc::Receiver<QueuedEntry>,
    reason: &str,
) {
    while let Ok(entry) = receiver.try_recv() {
        mark_entry_not_sent(supervisor, &entry.prompt, reason);
    }
}

/// An entry leaves the line unsent: the registry says so, and an admitted
/// API prompt's operation is settled by its one completion owner.
fn mark_entry_not_sent(
    supervisor: &ExternalProviderSupervisor,
    entry: &ProviderQueuedPrompt,
    reason: impl Into<String>,
) {
    let reason = reason.into();
    let ProviderQueuedPrompt::Contents {
        request, admitted, ..
    } = entry;
    supervisor
        .queued_operation_registry()
        .mark_not_submitted(&request.operation_id, reason.clone());
    if let Some(admitted) = admitted {
        supervisor.complete_unsent_api_entry(
            admitted,
            &request.operation_id,
            &request.target,
            &reason,
        );
    }
}

#[cfg(test)]
#[path = "provider_acp_message_fifo_test.rs"]
mod loaded_only_tests;
