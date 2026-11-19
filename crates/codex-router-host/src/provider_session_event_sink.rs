//! Ordered Host publication into the Router-owned provider Session hub.

use provider_session_machines::SessionGeneration;
use provider_session_machines::session_run_claims::RunClaim;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use acp_client_runtime::{
    EventSinkClosed, GenerationPublication, HistoryReplayFuture, HistoryReplayUnavailable,
    SessionEventSink, SessionRuns,
};
use collaboration_runtime::ProviderSessionEventHub;
use message_board::{SessionEndpointRef, SessionId, SessionRef};
use session_event_model::{SessionEvent, SessionItemKind};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Called once for every live event published for a session, in order.
/// Replayed history is not passed on.
pub(crate) trait LiveSessionEventObserver: Send + Sync + 'static {
    fn observe_live_event(&self, session_id: &str, event: &SessionEvent);
}

enum HubCommand {
    SkillView {
        session: SessionRef,
        view: session_event_model::SkillView,
    },
    Publish {
        session: SessionRef,
        event: SessionEvent,
        completion: Option<oneshot::Sender<Result<(), EventSinkClosed>>>,
    },
    BeginReplay {
        session: SessionRef,
        completion: oneshot::Sender<Result<(), HistoryReplayUnavailable>>,
    },
    BeginUnavailable {
        session: SessionRef,
        completion: oneshot::Sender<Result<(), HistoryReplayUnavailable>>,
    },
}

/// Each session has its own queue and consumer (design note §11.7, M5): a
/// session held at its hub publication never holds up another's. The
/// synchronous ACP reader only enqueues; a session's replay waits behind its
/// own earlier publications.
/// Every publication claims a run, and the connection's session cells decide
/// it: the claim is checked and the publication enqueued under the session
/// cell's lock, so nothing of a run lands after its terminal fact (D1 step
/// 6). The sink keeps no generations of its own.
pub(crate) struct HubSessionEventSink {
    endpoint: SessionEndpointRef,
    /// The connection's session cells, once the connection has attached
    /// them; until then the sink is a plain pipe.
    runs: StdMutex<Option<Arc<dyn SessionRuns>>>,
    hub: Arc<ProviderSessionEventHub>,
    /// Spawns each session's consumer; enqueues can come from a thread
    /// outside the runtime (the synchronous ACP reader).
    spawner: tokio::runtime::Handle,
    /// `None` once the sink has shut down.
    queues: StdMutex<Option<SessionQueues>>,
    not_publishing: Arc<StdMutex<HashSet<String>>>,
    catalog_refresh: StdMutex<Option<mpsc::UnboundedSender<String>>>,
    /// Told of every live event after the hub published it; installed after startup.
    live_observer: Arc<StdMutex<Option<Arc<dyn LiveSessionEventObserver>>>>,
    #[cfg(test)]
    reject_next_skill_view: std::sync::atomic::AtomicBool,
}

impl HubSessionEventSink {
    pub(crate) fn new(hub: Arc<ProviderSessionEventHub>, endpoint: SessionEndpointRef) -> Self {
        Self {
            endpoint,
            hub,
            spawner: tokio::runtime::Handle::current(),
            runs: StdMutex::new(None),
            queues: StdMutex::new(Some(SessionQueues::default())),
            not_publishing: Arc::new(StdMutex::new(HashSet::new())),
            catalog_refresh: StdMutex::new(None),
            live_observer: Arc::new(StdMutex::new(None)),
            #[cfg(test)]
            reject_next_skill_view: std::sync::atomic::AtomicBool::new(false),
        }
    }

    #[cfg(test)]
    pub(crate) fn reject_next_skill_view_for_test(&self) {
        self.reject_next_skill_view
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn skill_view_rejection_pending_for_test(&self) -> bool {
        self.reject_next_skill_view
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Every later live event of every session on this endpoint reaches `observer`.
    pub(crate) fn install_live_event_observer(&self, observer: Arc<dyn LiveSessionEventObserver>) {
        *self
            .live_observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(observer);
    }

    fn session(&self, session_id: &str) -> Result<SessionRef, HistoryReplayUnavailable> {
        let session_id =
            SessionId::try_from(session_id.to_owned()).map_err(|_| HistoryReplayUnavailable)?;
        Ok(SessionRef {
            endpoint: self.endpoint.clone(),
            session_id,
        })
    }

    /// Enqueues `commands` together when the session's cell admits `claim`;
    /// Ended, with nothing enqueued, when it doesn't. The check and the
    /// enqueue happen under the cell's lock; the enqueue is a push onto the
    /// unbounded pipe and never waits.
    fn enqueue_for(
        &self,
        session_id: &str,
        claim: RunClaim,
        commands: Vec<HubCommand>,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let runs = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut commands = Some(commands);
        let mut enqueue = || {
            for command in commands.take().unwrap_or_default() {
                self.enqueue(session_id, command)?;
            }
            Ok(())
        };
        let published = match runs {
            Some(runs) => runs.publish_if(session_id, claim, &mut enqueue)?,
            None => enqueue().map(|()| GenerationPublication::Published)?,
        };
        if published == GenerationPublication::Ended {
            tracing::debug!(session_id, "provider Session event dropped: its run ended");
        }
        Ok(published)
    }

    fn enqueue(&self, session_id: &str, command: HubCommand) -> Result<(), EventSinkClosed> {
        if self
            .not_publishing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(session_id)
        {
            return Err(EventSinkClosed);
        }
        let ends_queue = matches!(
            &command,
            HubCommand::Publish {
                event: SessionEvent::StateChanged {
                    state: session_event_model::SessionState::Closed,
                },
                ..
            }
        );
        let mut queues = self
            .queues
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sent = queues.as_mut().is_some_and(|queues| {
            let queue = queues.open_for(session_id, |previous, finished| {
                let (sender, receiver) = mpsc::unbounded_channel();
                let backlog = Arc::new(AtomicUsize::new(0));
                let consumer = self.spawner.spawn(run_session_consumer(
                    Arc::clone(&self.hub),
                    receiver,
                    Arc::clone(&backlog),
                    Arc::clone(&self.not_publishing),
                    Arc::clone(&self.live_observer),
                    previous,
                    finished,
                ));
                (
                    SessionQueue {
                        sender,
                        backlog,
                        backlog_warned: false,
                    },
                    consumer,
                )
            });
            queue.backlog.fetch_add(1, Ordering::Relaxed);
            if queue.sender.send(command).is_err() {
                queue.backlog.fetch_sub(1, Ordering::Relaxed);
                return false;
            }
            if queue.backlog.load(Ordering::Relaxed) > 10_000 && !queue.backlog_warned {
                queue.backlog_warned = true;
                tracing::warn!(session_id, "provider Session event backlog exceeds 10000");
            }
            if ends_queue {
                // The run's Closed is its queue's last command; a later run
                // gets a new queue, whose consumer waits for this one.
                queues.open.remove(session_id);
            }
            true
        });
        drop(queues);
        if !sent {
            self.not_publishing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(session_id.to_owned());
            tracing::error!(session_id, "provider Session event consumer is unavailable");
            return Err(EventSinkClosed);
        }
        Ok(())
    }

    pub(crate) async fn shutdown(&self) -> Result<(), HistoryReplayUnavailable> {
        self.catalog_refresh
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let queues = self
            .queues
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        // Every session's sender goes with its queue; each consumer drains
        // what it holds and ends.
        let consumers = queues.map(SessionQueues::close).unwrap_or_default();
        for consumer in consumers {
            consumer.await.map_err(|_| HistoryReplayUnavailable)?;
        }
        Ok(())
    }

    pub(crate) fn install_catalog_refresh(&self, sender: mpsc::UnboundedSender<String>) {
        *self
            .catalog_refresh
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sender);
    }

    /// A resumed session's history is invalidated, in its generation.
    pub(crate) async fn begin_history_unavailable(
        &self,
        session_id: &str,
        generation: SessionGeneration,
    ) -> Result<GenerationPublication, HistoryReplayUnavailable> {
        let session = self.session(session_id)?;
        let (completion, result) = oneshot::channel();
        let published = self
            .enqueue_for(
                session_id,
                RunClaim::Exact(generation),
                vec![HubCommand::BeginUnavailable {
                    session,
                    completion,
                }],
            )
            .map_err(|_| HistoryReplayUnavailable)?;
        if published == GenerationPublication::Ended {
            return Ok(published);
        }
        result.await.map_err(|_| HistoryReplayUnavailable)??;
        Ok(published)
    }

    /// A started, loaded or resumed session becomes active, in its
    /// generation: its capabilities, then Idle, enqueued together.
    pub(crate) async fn publish_activation(
        &self,
        session_id: &str,
        generation: SessionGeneration,
        capabilities: session_event_model::CapabilityReport,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        let (completion, result) = oneshot::channel();
        let published = self.enqueue_for(
            session_id,
            RunClaim::Exact(generation),
            vec![
                HubCommand::Publish {
                    session: session.clone(),
                    event: SessionEvent::CapabilitiesChanged { capabilities },
                    completion: None,
                },
                HubCommand::Publish {
                    session,
                    event: SessionEvent::StateChanged {
                        state: session_event_model::SessionState::Idle,
                    },
                    completion: Some(completion),
                },
            ],
        )?;
        if published == GenerationPublication::Ended {
            return Ok(published);
        }
        result.await.map_err(|_| EventSinkClosed)??;
        Ok(published)
    }
}

/// One session's open queue.
struct SessionQueue {
    sender: mpsc::UnboundedSender<HubCommand>,
    backlog: Arc<AtomicUsize>,
    backlog_warned: bool,
}

/// Every session's queue, and every consumer the sink has started.
#[derive(Default)]
struct SessionQueues {
    open: HashMap<String, SessionQueue>,
    /// For each session, a signal its latest consumer drops when it ends.
    latest_finished: HashMap<String, oneshot::Receiver<()>>,
    consumers: Vec<JoinHandle<()>>,
}

impl SessionQueues {
    /// The session's open queue, starting one (and its consumer) if needed.
    fn open_for(
        &mut self,
        session_id: &str,
        start: impl FnOnce(
            Option<oneshot::Receiver<()>>,
            oneshot::Sender<()>,
        ) -> (SessionQueue, JoinHandle<()>),
    ) -> &mut SessionQueue {
        match self.open.entry(session_id.to_owned()) {
            std::collections::hash_map::Entry::Occupied(open) => open.into_mut(),
            std::collections::hash_map::Entry::Vacant(vacant) => {
                let previous = self.latest_finished.remove(session_id);
                let (finished, finished_signal) = oneshot::channel();
                let (queue, consumer) = start(previous, finished);
                self.latest_finished
                    .insert(session_id.to_owned(), finished_signal);
                self.consumers.retain(|consumer| !consumer.is_finished());
                self.consumers.push(consumer);
                vacant.insert(queue)
            }
        }
    }

    /// Drops every sender and answers the consumers to wait for.
    fn close(self) -> Vec<JoinHandle<()>> {
        drop(self.open);
        self.consumers
    }
}

/// One session's consumer: it waits for that session's previous consumer
/// to finish, so a run's Closed is applied before anything of a later run,
/// then applies the session's commands in order. Sessions never wait on each
/// other (design note §11.7, M5).
async fn run_session_consumer(
    hub: Arc<ProviderSessionEventHub>,
    mut receiver: mpsc::UnboundedReceiver<HubCommand>,
    backlog: Arc<AtomicUsize>,
    failures: Arc<StdMutex<HashSet<String>>>,
    observer: Arc<StdMutex<Option<Arc<dyn LiveSessionEventObserver>>>>,
    previous: Option<oneshot::Receiver<()>>,
    _finished: oneshot::Sender<()>,
) {
    if let Some(previous) = previous {
        let _finished_before = previous.await;
    }
    // Sessions whose history is being replayed: those events are not live.
    let mut replaying: HashSet<String> = HashSet::new();
    while let Some(command) = receiver.recv().await {
        backlog.fetch_sub(1, Ordering::Relaxed);
        match command {
            HubCommand::SkillView { session, view } => {
                if hub.publish_skill_view(session.clone(), view).await.is_err() {
                    failures
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(session.session_id.as_str().to_owned());
                }
            }
            HubCommand::Publish {
                session,
                event,
                completion,
            } => {
                let session_closed = matches!(
                    &event,
                    SessionEvent::StateChanged {
                        state: session_event_model::SessionState::Closed,
                    }
                );
                // Only the small turn and state events matter to an observer.
                let observed = matches!(
                    &event,
                    SessionEvent::TurnStarted { .. }
                        | SessionEvent::InputAccepted { .. }
                        | SessionEvent::TurnEnded { .. }
                        | SessionEvent::StateChanged { .. }
                )
                .then(|| event.clone());
                let result = hub.publish(session.clone(), event).await.map(|_| ()).map_err(|error| {
                    tracing::error!(%error, session_id = %session.session_id.as_str(), "provider Session event publication failed");
                    failures
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(session.session_id.as_str().to_owned());
                    EventSinkClosed
                });
                if result.is_ok()
                    && let Some(observed) = &observed
                {
                    observe_live_event(
                        &observer,
                        &mut replaying,
                        session.session_id.as_str(),
                        observed,
                    );
                }
                if session_closed && result.is_ok() {
                    let session_id = session.session_id.as_str();
                    failures
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(session_id);
                }
                if let Some(completion) = completion {
                    let _ = completion.send(result);
                }
            }
            HubCommand::BeginReplay {
                session,
                completion,
            } => {
                let result = hub
                    .begin_history_replay(session.clone())
                    .await
                    .map(|_| ())
                    .map_err(|error| {
                        tracing::error!(%error, session_id = %session.session_id.as_str(), "provider Session replay reset failed");
                        HistoryReplayUnavailable
                    });
                if result.is_ok() {
                    replaying.insert(session.session_id.as_str().to_owned());
                }
                let _ = completion.send(result);
            }
            HubCommand::BeginUnavailable {
                session,
                completion,
            } => {
                let result = hub
                    .begin_history_unavailable(session.clone())
                    .await
                    .map(|_| ())
                    .map_err(|error| {
                        tracing::error!(%error, session_id = %session.session_id.as_str(), "provider Session history invalidation failed");
                        HistoryReplayUnavailable
                    });
                let _ = completion.send(result);
            }
        }
    }
}

/// Passes a published event to the observer unless it belongs to replayed history.
/// Replay ends at the first state change out of the replay's `Unloaded` state.
fn observe_live_event(
    observer: &StdMutex<Option<Arc<dyn LiveSessionEventObserver>>>,
    replaying: &mut HashSet<String>,
    session_id: &str,
    event: &SessionEvent,
) {
    if replaying.contains(session_id) {
        match event {
            SessionEvent::StateChanged {
                state: session_event_model::SessionState::Unloaded,
            } => return,
            SessionEvent::StateChanged { .. } => {
                replaying.remove(session_id);
            }
            _ => return,
        }
    }
    let observer = observer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(observer) = observer {
        observer.observe_live_event(session_id, event);
    }
}

impl HubSessionEventSink {
    fn begin_replay(&self, session_id: &str, claim: RunClaim) -> HistoryReplayFuture<'_> {
        let session = self.session(session_id);
        let session_id = session_id.to_owned();
        Box::pin(async move {
            let session = session?;
            let (completion, result) = oneshot::channel();
            // A load opens its generation before the replay begins, so an
            // ended one here means no load owns this session.
            let published = self
                .enqueue_for(
                    &session_id,
                    claim,
                    vec![HubCommand::BeginReplay {
                        session,
                        completion,
                    }],
                )
                .map_err(|_| HistoryReplayUnavailable)?;
            if published == GenerationPublication::Ended {
                return Err(HistoryReplayUnavailable);
            }
            result.await.map_err(|_| HistoryReplayUnavailable)?
        })
    }

    fn request_catalog_refresh(&self, session_id: &str) {
        if let Some(sender) = self
            .catalog_refresh
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            let _ = sender.send(session_id.to_owned());
        }
    }
}

impl SessionEventSink for HubSessionEventSink {
    fn publish_skill_view(
        &self,
        session_id: &str,
        view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        self.enqueue_for(
            session_id,
            RunClaim::Current,
            vec![HubCommand::SkillView { session, view }],
        )
        .map(|_| ())
    }

    fn begin_history_replay(&self, session_id: &str) -> HistoryReplayFuture<'_> {
        self.begin_replay(session_id, RunClaim::Current)
    }

    fn publish(&self, session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        let changes_config = matches!(
            &event,
            SessionEvent::ItemStarted { item }
                | SessionEvent::ItemUpdated { item }
                if matches!(item.kind, SessionItemKind::ConfigChange)
        );
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        // Dropped after its generation's Closed: the caller isn't told, so a
        // late event never tears down the connection.
        let published = self.enqueue_for(
            session_id,
            RunClaim::Current,
            vec![HubCommand::Publish {
                session,
                event,
                completion: None,
            }],
        )?;
        if changes_config && published == GenerationPublication::Published {
            self.request_catalog_refresh(session_id);
        }
        Ok(())
    }

    fn attach_session_runs(&self, runs: Arc<dyn SessionRuns>) {
        *self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(runs);
    }

    fn current_generation(&self, session_id: &str) -> SessionGeneration {
        self.runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map_or(SessionGeneration::UNTRACKED, |runs| {
                runs.current_generation(session_id)
            })
    }

    fn publish_in(
        &self,
        session_id: &str,
        generation: SessionGeneration,
        event: SessionEvent,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let changes_config = matches!(
            &event,
            SessionEvent::ItemStarted { item }
                | SessionEvent::ItemUpdated { item }
                if matches!(item.kind, SessionItemKind::ConfigChange)
        );
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        let published = self.enqueue_for(
            session_id,
            RunClaim::Exact(generation),
            vec![HubCommand::Publish {
                session,
                event,
                completion: None,
            }],
        )?;
        if changes_config && published == GenerationPublication::Published {
            self.request_catalog_refresh(session_id);
        }
        Ok(published)
    }

    fn publish_skill_view_in(
        &self,
        session_id: &str,
        generation: SessionGeneration,
        view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        #[cfg(test)]
        if self
            .reject_next_skill_view
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(EventSinkClosed);
        }
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        self.enqueue_for(
            session_id,
            RunClaim::Exact(generation),
            vec![HubCommand::SkillView { session, view }],
        )
        .map(|_| ())
    }

    fn begin_history_replay_in(
        &self,
        session_id: &str,
        generation: SessionGeneration,
    ) -> HistoryReplayFuture<'_> {
        self.begin_replay(session_id, RunClaim::Exact(generation))
    }

    /// Closed, `generation`'s terminal fact (or, for a session never
    /// admitted here, the last run's).
    fn publish_closed(
        &self,
        session_id: &str,
        generation: Option<SessionGeneration>,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let generation = generation.unwrap_or_else(|| self.current_generation(session_id));
        self.publish_terminal(
            session_id,
            generation,
            session_event_model::SessionState::Closed,
        )
    }

    /// Unloaded, `generation`'s terminal fact after its actor exited (S2).
    fn publish_unloaded(
        &self,
        session_id: &str,
        generation: SessionGeneration,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.publish_terminal(
            session_id,
            generation,
            session_event_model::SessionState::Unloaded,
        )
    }
}

impl HubSessionEventSink {
    /// A run's terminal state, which passes only once the session has ended
    /// with that run as its last.
    fn publish_terminal(
        &self,
        session_id: &str,
        generation: SessionGeneration,
        state: session_event_model::SessionState,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        self.enqueue_for(
            session_id,
            RunClaim::Terminal(generation),
            vec![HubCommand::Publish {
                session,
                event: SessionEvent::StateChanged { state },
                completion: None,
            }],
        )
    }
}

#[cfg(test)]
#[path = "provider_session_event_sink_test.rs"]
mod tests;

#[cfg(test)]
#[path = "provider_session_event_sink/lifecycle_test.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "provider_session_event_sink/generation_test.rs"]
mod generation_tests;
