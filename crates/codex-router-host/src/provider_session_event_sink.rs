//! Ordered Host publication into the Router-owned provider Session hub.

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use acp_client_runtime::{
    EventSinkClosed, HistoryReplayFuture, HistoryReplayUnavailable, SessionEventSink,
};
use collaboration_runtime::ProviderSessionEventHub;
use message_board::{SessionEndpointRef, SessionId, SessionRef};
use session_event_model::{SessionEvent, SessionItemKind};
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
};

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

/// One provider connection has one queue and one consumer. The synchronous ACP
/// reader only enqueues; replay waits behind every earlier publication.
pub(crate) struct HubSessionEventSink {
    endpoint: SessionEndpointRef,
    sender: StdMutex<Option<mpsc::UnboundedSender<HubCommand>>>,
    consumer: Mutex<Option<JoinHandle<()>>>,
    backlog: Arc<AtomicUsize>,
    backlog_warned: Arc<StdMutex<HashSet<String>>>,
    not_publishing: Arc<StdMutex<HashSet<String>>>,
    catalog_refresh: StdMutex<Option<mpsc::UnboundedSender<String>>>,
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
        let consumer = tokio::spawn(async move {
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
                        let result = hub.publish(session.clone(), event).await.map(|_| ()).map_err(|error| {
                            tracing::error!(%error, session_id = %session.session_id.as_str(), "provider Session event publication failed");
                            consumer_failures
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .insert(session.session_id.as_str().to_owned());
                            EventSinkClosed
                        });
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
            sender: StdMutex::new(Some(sender)),
            consumer: Mutex::new(Some(consumer)),
            backlog,
            backlog_warned,
            not_publishing,
            catalog_refresh: StdMutex::new(None),
        }
    }

    fn session(&self, session_id: &str) -> Result<SessionRef, HistoryReplayUnavailable> {
        let session_id =
            SessionId::try_from(session_id.to_owned()).map_err(|_| HistoryReplayUnavailable)?;
        Ok(SessionRef {
            endpoint: self.endpoint.clone(),
            session_id,
        })
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

    pub(crate) async fn begin_history_unavailable(
        &self,
        session_id: &str,
    ) -> Result<(), HistoryReplayUnavailable> {
        let session = self.session(session_id)?;
        let (completion, result) = oneshot::channel();
        self.enqueue(
            session_id,
            HubCommand::BeginUnavailable {
                session,
                completion,
            },
        )
        .map_err(|_| HistoryReplayUnavailable)?;
        result.await.map_err(|_| HistoryReplayUnavailable)?
    }

    pub(crate) async fn publish_idle(&self, session_id: &str) -> Result<(), EventSinkClosed> {
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        let (completion, result) = oneshot::channel();
        self.enqueue(
            session_id,
            HubCommand::Publish {
                session,
                event: SessionEvent::StateChanged {
                    state: session_event_model::SessionState::Idle,
                },
                completion: Some(completion),
            },
        )?;
        result.await.map_err(|_| EventSinkClosed)?
    }
}

impl SessionEventSink for HubSessionEventSink {
    fn publish_skill_view(
        &self,
        session_id: &str,
        view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        self.enqueue(session_id, HubCommand::SkillView { session, view })
    }

    fn begin_history_replay(&self, session_id: &str) -> HistoryReplayFuture<'_> {
        let session = self.session(session_id);
        let session_id = session_id.to_owned();
        Box::pin(async move {
            let session = session?;
            let (completion, result) = oneshot::channel();
            self.enqueue(
                &session_id,
                HubCommand::BeginReplay {
                    session,
                    completion,
                },
            )
            .map_err(|_| HistoryReplayUnavailable)?;
            result.await.map_err(|_| HistoryReplayUnavailable)?
        })
    }

    fn publish(&self, session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        let changes_config = matches!(
            &event,
            SessionEvent::ItemStarted { item }
                | SessionEvent::ItemUpdated { item }
                if matches!(item.kind, SessionItemKind::ConfigChange)
        );
        let session = self.session(session_id).map_err(|_| EventSinkClosed)?;
        self.enqueue(
            session_id,
            HubCommand::Publish {
                session,
                event,
                completion: None,
            },
        )?;
        if changes_config
            && let Some(sender) = self
                .catalog_refresh
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
        {
            let _ = sender.send(session_id.to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "provider_session_event_sink_test.rs"]
mod tests;

#[cfg(test)]
#[path = "provider_session_event_sink/lifecycle_test.rs"]
mod lifecycle_tests;
