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

pub(crate) struct ProviderAcpMessageFifo {
    #[cfg(test)]
    wait_started: Arc<tokio::sync::Notify>,
    supervisor: Arc<ExternalProviderSupervisor>,
    store: Arc<Mutex<ProviderOperationStore>>,
    ownership: Arc<dyn LiveSessionOwnershipCheck>,
    senders: StdMutex<HashMap<SessionRef, mpsc::Sender<ProviderQueuedPrompt>>>,
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
            workers: StdMutex::new(Vec::new()),
            shutdown: CancellationToken::new(),
        }
    }

    pub(crate) fn reserve(
        &self,
        target: &SessionRef,
    ) -> Result<mpsc::OwnedPermit<ProviderQueuedPrompt>, &'static str> {
        if self.shutdown.is_cancelled() {
            return Err("provider queue is shutting down");
        }
        let mut senders = self
            .senders
            .lock()
            .map_err(|_| "provider queue unavailable")?;
        let sender = if let Some(sender) = senders.get(target).filter(|sender| !sender.is_closed())
        {
            sender.clone()
        } else {
            if senders.len() >= MAX_SESSION_QUEUES {
                return Err("provider session queue capacity exceeded");
            }
            let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
            let worker = tokio::spawn(run_provider_message_fifo(
                target.clone(),
                receiver,
                Arc::clone(&self.supervisor),
                Arc::clone(&self.store),
                Arc::clone(&self.ownership),
                self.shutdown.clone(),
                #[cfg(test)]
                Arc::clone(&self.wait_started),
            ));
            self.workers
                .lock()
                .map_err(|_| "provider queue workers unavailable")?
                .push(worker);
            senders.insert(target.clone(), sender.clone());
            sender
        };
        sender
            .try_reserve_owned()
            .map_err(|_| "provider session queue is full")
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
            _ => Vec::new(),
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
        permit.send(ProviderQueuedPrompt::Contents {
            request,
            load_policy: LoadPolicy::MayLoad,
            admitted: Some(admitted),
        });
        Ok(queued)
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
    mut receiver: mpsc::Receiver<ProviderQueuedPrompt>,
    supervisor: Arc<ExternalProviderSupervisor>,
    store: Arc<Mutex<ProviderOperationStore>>,
    ownership: Arc<dyn LiveSessionOwnershipCheck>,
    shutdown: CancellationToken,
    #[cfg(test)] wait_started: Arc<tokio::sync::Notify>,
) {
    loop {
        let request = tokio::select! {
            () = shutdown.cancelled() => {
                mark_remaining_not_submitted(&supervisor, &mut receiver, "Router shutdown before provider submission");
                return;
            },
            request = receiver.recv() => match request {
                Some(request) => request,
                None => return,
            },
        };
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
    receiver: &mut mpsc::Receiver<ProviderQueuedPrompt>,
    reason: &str,
) {
    mark_entry_not_sent(supervisor, current, reason);
    mark_remaining_not_submitted(supervisor, receiver, reason);
}

fn mark_remaining_not_submitted(
    supervisor: &ExternalProviderSupervisor,
    receiver: &mut mpsc::Receiver<ProviderQueuedPrompt>,
    reason: &str,
) {
    while let Ok(request) = receiver.try_recv() {
        mark_entry_not_sent(supervisor, &request, reason);
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
