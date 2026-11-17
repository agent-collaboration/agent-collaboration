//! Ordered Host publication into the Router-owned provider Session hub.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use acp_client_runtime::{
    EventSinkClosed, GenerationPublication, HistoryReplayFuture, HistoryReplayUnavailable,
    SessionEventSink, SessionGeneration,
};
use collaboration_runtime::ProviderSessionEventHub;
use message_board::{SessionEndpointRef, SessionId, SessionRef};
use session_event_model::{SessionEvent, SessionItemKind};
use tokio::{
    sync::{Mutex, mpsc, oneshot},
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

/// A session's current run at this sink (D1 R14). The connection opens a
/// generation when it admits a create, load or resume, and ends it with that
/// run's Closed; a publication from any other generation is dropped.
#[derive(Default)]
struct GenerationState {
    current: u64,
    closed: bool,
}

/// The generation a publication belongs to.
#[derive(Clone, Copy)]
enum GenerationClaim {
    /// Whatever is current, unless it is closed: connection-wide
    /// publishers only (auth status, provider exit).
    Current,
    /// Exactly this one, still open: a runtime continuation.
    Exact(SessionGeneration),
    /// Exactly this one, still open; publishing it closes it.
    Closing(SessionGeneration),
}

/// One provider connection has one queue and one consumer. The synchronous ACP
/// reader only enqueues; replay waits behind every earlier publication.
/// Every enqueue is checked against its session's generation under one lock,
/// so a generation's Closed is ordered against everything published for it.
pub(crate) struct HubSessionEventSink {
    endpoint: SessionEndpointRef,
    generations: StdMutex<HashMap<String, GenerationState>>,
    sender: StdMutex<Option<mpsc::UnboundedSender<HubCommand>>>,
    consumer: Mutex<Option<JoinHandle<()>>>,
    backlog: Arc<AtomicUsize>,
    backlog_warned: Arc<StdMutex<HashSet<String>>>,
    not_publishing: Arc<StdMutex<HashSet<String>>>,
    catalog_refresh: StdMutex<Option<mpsc::UnboundedSender<String>>>,
    /// Told of every live event after the hub published it; installed after startup.
    live_observer: Arc<StdMutex<Option<Arc<dyn LiveSessionEventObserver>>>>,
}

impl HubSessionEventSink {
    pub(crate) fn new(hub: Arc<ProviderSessionEventHub>, endpoint: SessionEndpointRef) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let backlog = Arc::new(AtomicUsize::new(0));
        let backlog_warned = Arc::new(StdMutex::new(HashSet::new()));
        let not_publishing = Arc::new(StdMutex::new(HashSet::new()));
        let consumer_backlog = Arc::clone(&backlog);
        let consumer_warned = Arc::clone(&backlog_warned);
        let consumer_failures = Arc::clone(&not_publishing);
        let live_observer: Arc<StdMutex<Option<Arc<dyn LiveSessionEventObserver>>>> =
            Arc::new(StdMutex::new(None));
        let consumer_observer = Arc::clone(&live_observer);
        let consumer = tokio::spawn(async move {
            // Sessions whose history is being replayed: those events are not live.
            let mut replaying: HashSet<String> = HashSet::new();
            while let Some(command) = receiver.recv().await {
                consumer_backlog.fetch_sub(1, Ordering::Relaxed);
                match command {
                    HubCommand::SkillView { session, view } => {
                        if hub.publish_skill_view(session.clone(), view).await.is_err() {
                            consumer_failures
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
                            consumer_failures
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .insert(session.session_id.as_str().to_owned());
                            EventSinkClosed
                        });
                        if result.is_ok()
                            && let Some(observed) = &observed
                        {
                            observe_live_event(
                                &consumer_observer,
                                &mut replaying,
                                session.session_id.as_str(),
                                observed,
                            );
                        }
                        if session_closed && result.is_ok() {
                            let session_id = session.session_id.as_str();
                            consumer_warned
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .remove(session_id);
                            consumer_failures
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
        });
        Self {
            endpoint,
            generations: StdMutex::new(HashMap::new()),
            sender: StdMutex::new(Some(sender)),
            consumer: Mutex::new(Some(consumer)),
            backlog,
            backlog_warned,
            not_publishing,
            catalog_refresh: StdMutex::new(None),
            live_observer,
        }
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

    /// Enqueues `commands` together when `claim` holds; Ended, with nothing
    /// enqueued, when its generation is closed or no longer current.
    fn enqueue_for(
        &self,
        session_id: &str,
        claim: GenerationClaim,
        commands: Vec<HubCommand>,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = generations.entry(session_id.to_owned()).or_default();
        let holds = match claim {
            GenerationClaim::Current => !state.closed,
            GenerationClaim::Exact(generation) | GenerationClaim::Closing(generation) => {
                !state.closed && generation.number() == state.current
            }
        };
        if !holds {
            tracing::debug!(
                session_id,
                "provider Session event dropped: its generation ended"
            );
            return Ok(GenerationPublication::Ended);
        }
        for command in commands {
            self.enqueue(session_id, command)?;
        }
        if matches!(claim, GenerationClaim::Closing(_)) {
            state.closed = true;
        }
        Ok(GenerationPublication::Published)
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
        self.backlog.fetch_add(1, Ordering::Relaxed);
        let sent = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|sender| sender.send(command).is_ok());
        if !sent {
            self.backlog.fetch_sub(1, Ordering::Relaxed);
            self.not_publishing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(session_id.to_owned());
            tracing::error!(session_id, "provider Session event consumer is unavailable");
            return Err(EventSinkClosed);
        }
        if self.backlog.load(Ordering::Relaxed) > 10_000
            && self
                .backlog_warned
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(session_id.to_owned())
        {
            tracing::warn!(session_id, "provider Session event backlog exceeds 10000");
        }
        Ok(())
    }

    pub(crate) async fn shutdown(&self) -> Result<(), HistoryReplayUnavailable> {
        self.catalog_refresh
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(consumer) = self.consumer.lock().await.take() {
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
                GenerationClaim::Exact(generation),
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
            GenerationClaim::Exact(generation),
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
    fn begin_replay(&self, session_id: &str, claim: GenerationClaim) -> HistoryReplayFuture<'_> {
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
            GenerationClaim::Current,
            vec![HubCommand::SkillView { session, view }],
        )
        .map(|_| ())
    }

    fn begin_history_replay(&self, session_id: &str) -> HistoryReplayFuture<'_> {
        self.begin_replay(session_id, GenerationClaim::Current)
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
            GenerationClaim::Current,
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

    fn open_generation(&self, session_id: &str) -> SessionGeneration {
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = generations.entry(session_id.to_owned()).or_default();
        state.current = state.current.wrapping_add(1);
        state.closed = false;
        SessionGeneration::from_number(state.current)
    }

    fn adopt_generation(&self, session_id: &str, generation: SessionGeneration) {
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = generations.entry(session_id.to_owned()).or_default();
        state.current = generation.number();
        state.closed = false;
    }

    fn current_generation(&self, session_id: &str) -> SessionGeneration {
        SessionGeneration::from_number(
            self.generations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(session_id)
                .map_or(0, |state| state.current),
        )
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
            GenerationClaim::Exact(generation),
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
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        self.enqueue_for(
            session_id,
            GenerationClaim::Exact(generation),
            vec![HubCommand::SkillView { session, view }],
        )
        .map(|_| ())
    }

    fn begin_history_replay_in(
        &self,
        session_id: &str,
        generation: SessionGeneration,
    ) -> HistoryReplayFuture<'_> {
        self.begin_replay(session_id, GenerationClaim::Exact(generation))
    }

    /// Ends `generation` (or, for a session never admitted here, the
    /// current one) with Closed.
    fn publish_closed(
        &self,
        session_id: &str,
        generation: Option<SessionGeneration>,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let generation = generation.unwrap_or_else(|| self.current_generation(session_id));
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        self.enqueue_for(
            session_id,
            GenerationClaim::Closing(generation),
            vec![HubCommand::Publish {
                session,
                event: SessionEvent::StateChanged {
                    state: session_event_model::SessionState::Closed,
                },
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
