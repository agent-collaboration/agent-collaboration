//! Bounded, Host-lifetime FIFO for provider prompts accepted as queued (D1
//! R12), on each session cell's line (D1 state machines, design note §11.2,
//! §12.2, §13.1). The session's cell decides every move: a door admits a
//! message into the line (loading the session first when the message may
//! load it), the line's worker commits the head once the run is Live and no
//! other message is in flight, and a close, cancel, failed load, stop or
//! closed connection that wins ends the messages it reaches. The FIFO keeps
//! only the payloads, in number order, and settles each message once.

#[path = "provider_acp_fifo_entries.rs"]
mod entries;
use crate::provider_acp_session_loading::{
    ProviderSessionLoadability, inspect_provider_session_loadability, load_error_reason,
};
use crate::{
    ExternalProviderRuntimeError, ExternalProviderSupervisor, LiveSessionOwnershipCheck,
    external_provider_supervisor::ProviderLineRuntime,
    external_provider_supervisor::ProviderPromptContentsRequest,
};
use acp_client_runtime::{QueueEntrySink, QueueRefusal};
use collaboration_contracts::LoadPolicy;
use collaboration_protocol::{OperationId, SessionRef};
use collaboration_runtime::ProviderOperationStore;
use entries::{QueuedEntries, QueuedRecord};
use futures_util::FutureExt;
use futures_util::future::{BoxFuture, Shared};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const QUEUE_CAPACITY: usize = 128;
const MAX_SESSION_QUEUES: usize = 1024;
const SETTLEMENT_WAIT_SECONDS: u32 = 10;
const MIN_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);
/// The first retry wait of a line whose commit is refused for now: a scenario
/// test reads it to know the injected-clock deadline a waiting message
/// sleeps to (design note §11.8). Test support only.
#[cfg(feature = "test-support")]
pub const QUEUE_RETRY_DELAY: std::time::Duration = MIN_RETRY_DELAY;
const MAX_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);
/// Why a message ends not sent when its session closed (D1 R14).
pub(crate) const SESSION_CLOSED: &str = "session closed";
/// Why a message ends not sent when the Router stops its lines.
const ROUTER_SHUTDOWN: &str = "Router shutting down";
/// Why a message ends not sent when its provider connection closed.
const CONNECTION_CLOSED: &str = "provider connection closed";

#[derive(Clone)]
pub(crate) enum ProviderQueuedPrompt {
    Contents {
        request: ProviderPromptContentsRequest,
        /// An API prompt admitted before it was queued (D1 R12); mail and
        /// content-command entries are admitted when dispatched.
        admitted: Option<crate::external_provider_supervisor::AdmittedApiPrompt>,
    },
}

impl ProviderQueuedPrompt {
    pub(crate) fn operation_id(&self) -> &OperationId {
        match self {
            Self::Contents { request, .. } => &request.operation_id,
        }
    }
}

/// Why the line's door refused a message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QueueDoorRefusal {
    /// The line, or the Host's lines, are full, or the queue is stopping.
    Busy(&'static str),
    /// The session isn't loaded and the message may not load it (M8 answer).
    NotLoaded(String),
    /// The session's state refuses the message, or its load can't start;
    /// the answer says why.
    Refused(String),
}

/// A load a door started, shared by the line's worker and its driver.
type SharedLoad = Shared<BoxFuture<'static, Result<(), String>>>;

/// One session's line: the payloads in number order and its worker's state.
struct SessionLine {
    /// Stops this session's worker at Router shutdown.
    stop: CancellationToken,
    /// The one injected clock: the line's retries wait on it.
    clock: Arc<dyn router_clock::RuntimeClock>,
    /// Wakes the worker when a payload arrives.
    arrived: tokio::sync::Notify,
    pending: StdMutex<BTreeMap<u64, OperationId>>,
    /// Doors admitted into the line whose payload hasn't arrived yet.
    reserved: AtomicUsize,
    /// The worker holds a message (D1 R12: the 128 waiting count the held head).
    holding: AtomicBool,
    load: StdMutex<Option<(u64, SharedLoad)>>,
    loads: AtomicU64,
    /// The last door load's failure, for messages it ended.
    last_load_failure: StdMutex<Option<String>>,
}

impl SessionLine {
    fn new(stop: CancellationToken, clock: Arc<dyn router_clock::RuntimeClock>) -> Self {
        Self {
            stop,
            clock,
            arrived: tokio::sync::Notify::new(),
            pending: StdMutex::new(BTreeMap::new()),
            reserved: AtomicUsize::new(0),
            holding: AtomicBool::new(false),
            load: StdMutex::new(None),
            loads: AtomicU64::new(0),
            last_load_failure: StdMutex::new(None),
        }
    }

    fn waiting(&self) -> usize {
        lock(&self.pending).len()
            + self.reserved.load(Ordering::SeqCst)
            + usize::from(self.holding.load(Ordering::SeqCst))
    }

    fn current_load(&self) -> Option<SharedLoad> {
        lock(&self.load).as_ref().map(|(_, load)| load.clone())
    }

    fn load_failure(&self) -> String {
        lock(&self.last_load_failure)
            .clone()
            .unwrap_or_else(|| "the session couldn't be loaded".to_owned())
    }
}

/// A message the session's cell admitted; sending hands over its payload.
pub(crate) struct QueuePermit {
    line: Arc<SessionLine>,
    entries: Arc<QueuedEntries>,
    runtime: ProviderLineRuntime,
    target: SessionRef,
    operation_id: OperationId,
    sequence: u64,
    sent: bool,
}

impl QueuePermit {
    pub(crate) fn send(mut self, prompt: ProviderQueuedPrompt) {
        self.sent = true;
        let queued = self.entries.insert(QueuedRecord {
            target: self.target.clone(),
            runtime: self.runtime.clone(),
            prompt,
            admitted_at_head: false,
        });
        if queued {
            lock(&self.line.pending).insert(self.sequence, self.operation_id.clone());
        }
        self.line.reserved.fetch_sub(1, Ordering::SeqCst);
        self.line.arrived.notify_one();
    }
}

impl Drop for QueuePermit {
    fn drop(&mut self) {
        if !self.sent {
            // Never handed over: the message leaves the line unsent, and
            // nothing waits on it yet.
            let _cancelled = self.runtime.queue_cancel(
                &String::from(self.target.session_id.clone()),
                &String::from(self.operation_id.clone()),
            );
            self.line.reserved.fetch_sub(1, Ordering::SeqCst);
            self.line.arrived.notify_one();
        }
    }
}

pub(crate) struct ProviderAcpMessageFifo {
    /// The one injected clock, handed to each session line.
    clock: Arc<dyn router_clock::RuntimeClock>,
    #[cfg(test)]
    wait_started: Arc<tokio::sync::Notify>,
    supervisor: Arc<ExternalProviderSupervisor>,
    store: Arc<Mutex<ProviderOperationStore>>,
    ownership: Arc<dyn LiveSessionOwnershipCheck>,
    lines: StdMutex<HashMap<SessionRef, Arc<SessionLine>>>,
    entries: Arc<QueuedEntries>,
    workers: StdMutex<Vec<JoinHandle<()>>>,
    shutdown: CancellationToken,
}

impl ProviderAcpMessageFifo {
    pub(crate) fn new(
        supervisor: Arc<ExternalProviderSupervisor>,
        store: Arc<Mutex<ProviderOperationStore>>,
        ownership: Arc<dyn LiveSessionOwnershipCheck>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Self {
        Self {
            clock,
            #[cfg(test)]
            wait_started: Arc::new(tokio::sync::Notify::new()),
            entries: Arc::new(QueuedEntries::new(&supervisor)),
            supervisor,
            store,
            ownership,
            lines: StdMutex::new(HashMap::new()),
            workers: StdMutex::new(Vec::new()),
            shutdown: CancellationToken::new(),
        }
    }

    /// The door: admits `operation_id` into the session's line. A message
    /// that may load the session loads it first when it isn't loaded, so it
    /// joins the loading run's line in order; one that may not is refused
    /// with the session's state.
    pub(crate) async fn reserve(
        &self,
        target: &SessionRef,
        operation_id: &OperationId,
        load_policy: LoadPolicy,
    ) -> Result<QueuePermit, QueueDoorRefusal> {
        if self.shutdown.is_cancelled() {
            return Err(QueueDoorRefusal::Busy("provider queue is shutting down"));
        }
        let Some(runtime) = self.supervisor.line_runtime_for(&target.endpoint) else {
            return Err(QueueDoorRefusal::Refused(
                "provider runtime is unavailable".to_owned(),
            ));
        };
        runtime.set_queue_entry_sink(Arc::clone(&self.entries) as Arc<dyn QueueEntrySink>);
        let line = self.admit_to_line(target)?;
        let session_id = String::from(target.session_id.clone());
        let operation = String::from(operation_id.clone());
        let mut loaded = false;
        let admitted = loop {
            match runtime.queue_enqueue(&session_id, &operation) {
                Ok(ticket) => break Ok(ticket),
                Err(QueueRefusal::NotLoaded { .. })
                    if load_policy == LoadPolicy::MayLoad && !loaded =>
                {
                    loaded = true;
                    if let Err(refusal) = self.load_at_door(&runtime, target, &line).await {
                        break Err(refusal);
                    }
                }
                Err(QueueRefusal::NotLoaded { answer }) => {
                    break Err(QueueDoorRefusal::NotLoaded(answer));
                }
                Err(QueueRefusal::Refused { answer }) => {
                    break Err(QueueDoorRefusal::Refused(answer));
                }
                Err(QueueRefusal::NotYet | QueueRefusal::EntryEnded) => {
                    break Err(QueueDoorRefusal::Refused(
                        "provider session queue is unavailable".to_owned(),
                    ));
                }
            }
        };
        match admitted {
            Ok(ticket) => Ok(QueuePermit {
                line,
                entries: Arc::clone(&self.entries),
                runtime,
                target: target.clone(),
                operation_id: operation_id.clone(),
                sequence: ticket.sequence,
                sent: false,
            }),
            Err(refusal) => {
                line.reserved.fetch_sub(1, Ordering::SeqCst);
                Err(refusal)
            }
        }
    }

    /// Room in the session's line, counted against its 128 waiting; starts
    /// the line's worker the first time.
    fn admit_to_line(&self, target: &SessionRef) -> Result<Arc<SessionLine>, QueueDoorRefusal> {
        let mut lines = lock(&self.lines);
        if let Some(line) = lines.get(target) {
            if line.waiting() >= QUEUE_CAPACITY {
                return Err(QueueDoorRefusal::Busy("provider session queue is full"));
            }
            line.reserved.fetch_add(1, Ordering::SeqCst);
            return Ok(Arc::clone(line));
        }
        if lines.len() >= MAX_SESSION_QUEUES {
            return Err(QueueDoorRefusal::Busy(
                "provider session queue capacity exceeded",
            ));
        }
        let line = Arc::new(SessionLine::new(
            self.shutdown.child_token(),
            Arc::clone(&self.clock),
        ));
        line.reserved.fetch_add(1, Ordering::SeqCst);
        let worker = tokio::spawn(run_session_line(SessionLineWorker {
            target: target.clone(),
            line: Arc::clone(&line),
            entries: Arc::clone(&self.entries),
            supervisor: Arc::clone(&self.supervisor),
            #[cfg(test)]
            wait_started: Arc::clone(&self.wait_started),
        }));
        lock(&self.workers).push(worker);
        lines.insert(target.clone(), Arc::clone(&line));
        Ok(line)
    }

    /// Starts loading the session for a door: the record, ownership and
    /// load capability are checked first, as the worker did before; once
    /// the session's cell is Loading, the load finishes in the background
    /// and the line's worker commits nothing before it does.
    async fn load_at_door(
        &self,
        runtime: &ProviderLineRuntime,
        target: &SessionRef,
        line: &Arc<SessionLine>,
    ) -> Result<(), QueueDoorRefusal> {
        let provider_session_id = String::from(target.session_id.clone());
        // One mode operation at a time per session: the record read and the mode
        // the restore puts back wait for any explicit change still recording,
        // and a change waits for the restore (review F3).
        let _mode_turn = runtime
            .session_setup()
            .mode_order()
            .enter(&provider_session_id)
            .await;
        let (working_directory, setup) = match inspect_provider_session_loadability(
            runtime,
            &self.store,
            self.ownership.as_ref(),
            target,
        )
        .await
        {
            // Loaded meanwhile: the message joins the live run's line.
            ProviderSessionLoadability::AlreadyLoaded => return Ok(()),
            ProviderSessionLoadability::Loadable {
                working_directory,
                setup,
            } => (working_directory, setup),
            ProviderSessionLoadability::UnsupportedLoad => {
                return Err(QueueDoorRefusal::Refused("unsupported: load".to_owned()));
            }
            ProviderSessionLoadability::MissingRecord => {
                return Err(QueueDoorRefusal::Refused(
                    "provider session record is missing".to_owned(),
                ));
            }
            ProviderSessionLoadability::LiveElsewhere => {
                return Err(QueueDoorRefusal::Refused(
                    "session is live elsewhere".to_owned(),
                ));
            }
            ProviderSessionLoadability::Unavailable { reason } => {
                return Err(QueueDoorRefusal::Refused(reason));
            }
        };
        let finish = match runtime
            .begin_load_session(
                String::from(target.session_id.clone()),
                working_directory,
                setup,
            )
            .await
        {
            Ok(finish) => finish,
            // Another door's load is in flight: the message joins its line.
            Err(failure)
                if matches!(
                    failure.error(),
                    ExternalProviderRuntimeError::LocalBusy { .. }
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(QueueDoorRefusal::Refused(load_error_reason(&error))),
        };
        let load: SharedLoad = finish
            .map(|result| result.map_err(|error| load_error_reason(&error)))
            .boxed()
            .shared();
        let load_id = line.loads.fetch_add(1, Ordering::SeqCst);
        *lock(&line.load) = Some((load_id, load.clone()));
        // Drives the load whether or not a worker waits on it, then clears it.
        // The session's mode turn is held until the load has finished.
        let driven_line = Arc::clone(line);
        tokio::spawn(async move {
            let result = load.await;
            drop(_mode_turn);
            let mut current = lock(&driven_line.load);
            if current.as_ref().is_some_and(|(id, _)| *id == load_id) {
                *current = None;
            }
            drop(current);
            if let Err(reason) = result {
                *lock(&driven_line.last_load_failure) = Some(reason);
            }
            driven_line.arrived.notify_one();
        });
        Ok(())
    }

    /// Queues an admitted API prompt; refused when the session's line is
    /// full or its state refuses it.
    pub(crate) async fn enqueue_admitted_api_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        binding: collaboration_protocol::ProviderBindingIdentity,
        admitted: crate::external_provider_supervisor::AdmittedApiPrompt,
    ) -> Result<crate::ProviderQueuedInput, QueueDoorRefusal> {
        let permit = self
            .reserve(&request.target, &request.operation_id, LoadPolicy::MayLoad)
            .await?;
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
        permit.send(ProviderQueuedPrompt::Contents {
            request,
            admitted: Some(admitted),
        });
        Ok(queued)
    }

    /// Cancels a waiting message through its session's cell: a waiting one
    /// ends not sent at once, through its one completion owner; one whose
    /// send is being decided is withdrawn and its owner settles it.
    pub(crate) fn cancel_queued(
        &self,
        target: &SessionRef,
        input_id: &session_event_model::InputId,
    ) -> Result<(), crate::ProviderQueueCancellationError> {
        let registry = self.supervisor.queued_operation_registry();
        let operation_id = registry
            .queued_operation_id(target, input_id)
            .ok_or(crate::ProviderQueueCancellationError::NotQueued)?;
        let record = self
            .entries
            .get(&operation_id)
            .ok_or(crate::ProviderQueueCancellationError::NotQueued)?;
        match record.runtime.queue_cancel(
            &String::from(target.session_id.clone()),
            &String::from(operation_id.clone()),
        ) {
            Ok(acp_client_runtime::QueueCancel::NotSent { reason }) => {
                let _cancelled = registry.cancel(target, input_id);
                if let Some(record) = self.entries.take(&operation_id)
                    && let ProviderQueuedPrompt::Contents {
                        admitted: Some(admitted),
                        ..
                    } = &record.prompt
                {
                    self.supervisor.complete_unsent_api_entry(
                        admitted,
                        &operation_id,
                        target,
                        reason,
                    );
                }
                Ok(())
            }
            Ok(acp_client_runtime::QueueCancel::Withdrawing) => Ok(()),
            Err(_refused) => Err(crate::ProviderQueueCancellationError::NotQueued),
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.stop_lines();
        self.shutdown.cancel();
        let workers = std::mem::take(&mut *lock(&self.workers));
        for worker in workers {
            let _result = worker.await;
        }
    }

    /// Ends every line before its worker stops, so a message either
    /// committed its send already or ends not sent (D1 R14).
    fn stop_lines(&self) {
        let runtimes: Vec<ProviderLineRuntime> = {
            let lines = lock(&self.lines);
            lines
                .keys()
                .filter_map(|target| self.supervisor.line_runtime_for(&target.endpoint))
                .collect()
        };
        for runtime in runtimes {
            runtime.queue_stop();
        }
    }
}

impl Drop for ProviderAcpMessageFifo {
    fn drop(&mut self) {
        self.stop_lines();
        self.shutdown.cancel();
        for worker in lock(&self.workers).drain(..) {
            worker.abort();
        }
    }
}

fn lock<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[path = "provider_acp_fifo_worker.rs"]
mod worker;
use worker::{SessionLineWorker, run_session_line};

#[cfg(test)]
#[path = "provider_acp_message_fifo_test.rs"]
mod loaded_only_tests;

#[cfg(test)]
#[path = "provider_acp_message_fifo_refusal_test.rs"]
mod refusal_tests;

#[cfg(test)]
#[path = "provider_acp_message_fifo_dispatch_ownership_test.rs"]
mod dispatch_ownership_tests;

#[cfg(test)]
#[path = "provider_acp_message_fifo_router_close_test.rs"]
mod router_close_tests;
