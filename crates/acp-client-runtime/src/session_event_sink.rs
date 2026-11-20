//! Nonblocking publication boundary for Router-owned Session events.

use provider_session_machines::SessionGeneration;
use std::{future::Future, pin::Pin, sync::Arc};

use session_event_model::SessionEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("session event consumer closed")]
pub struct EventSinkClosed;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("session history replay could not begin")]
pub struct HistoryReplayUnavailable;

pub type HistoryReplayFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), HistoryReplayUnavailable>> + Send + 'a>>;

/// Whether a publication tied to a generation went out or found it ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationPublication {
    Published,
    /// The generation was closed, or a newer one had opened.
    Ended,
}

pub trait SessionEventSink: Send + Sync + 'static {
    fn publish_skill_view(
        &self,
        session_id: &str,
        view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed>;
    /// Reset hub history before the agent can publish replayed updates.
    fn begin_history_replay(&self, session_id: &str) -> HistoryReplayFuture<'_>;

    /// Return promptly; failure means the event consumer has closed.
    /// Without a generation: for connection-wide publishers only (auth
    /// status, provider exit), which speak for whatever run is current.
    fn publish(&self, session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed>;

    /// The connection's session cells, which own every run: a sink checks
    /// each publication's claim against them (D1 step 6). Called once the
    /// connection exists, before any session runs.
    fn attach_session_runs(&self, _runs: Arc<dyn crate::SessionRuns>) {}

    /// The session's generation now, for a callback starting work on it.
    fn current_generation(&self, _session_id: &str) -> SessionGeneration {
        SessionGeneration::UNTRACKED
    }

    /// Publishes an event produced under `generation`; dropped (Ended) once
    /// that generation is closed or superseded.
    fn publish_in(
        &self,
        session_id: &str,
        _generation: SessionGeneration,
        event: SessionEvent,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.publish(session_id, event)
            .map(|()| GenerationPublication::Published)
    }

    fn publish_for_turn(
        &self,
        session_id: &str,
        generation: SessionGeneration,
        _turn: provider_session_machines::session_turn_machine::TurnId,
        event: SessionEvent,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.publish_in(session_id, generation, event)
    }

    fn publish_connection_auth(
        &self,
        _session_id: &str,
        _status: session_event_model::ProviderAuthStatus,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }

    fn publish_skill_view_in(
        &self,
        session_id: &str,
        _generation: SessionGeneration,
        view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        self.publish_skill_view(session_id, view)
    }

    fn begin_history_replay_in(
        &self,
        session_id: &str,
        _generation: SessionGeneration,
    ) -> HistoryReplayFuture<'_> {
        self.begin_history_replay(session_id)
    }

    /// The session closed: Closed ends `generation` (the current one when
    /// the session was never admitted on this connection).
    fn publish_closed(
        &self,
        session_id: &str,
        _generation: Option<SessionGeneration>,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.publish(
            session_id,
            SessionEvent::StateChanged {
                state: session_event_model::SessionState::Closed,
            },
        )
        .map(|()| GenerationPublication::Published)
    }

    /// The session's actor exited on its own: Unloaded is `generation`'s
    /// terminal fact (S2).
    fn publish_unloaded(
        &self,
        session_id: &str,
        generation: SessionGeneration,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.publish_in(
            session_id,
            generation,
            SessionEvent::StateChanged {
                state: session_event_model::SessionState::Unloaded,
            },
        )
    }
}

/// One admitted session's view of the sink: every publication carries the
/// generation the session was admitted under, so a producer that outlives
/// its run (an actor, a projection, a settings update) can't publish into a
/// later one.
#[cfg(test)]
pub(crate) struct SessionGenerationSink {
    inner: Arc<dyn SessionEventSink>,
    generation: SessionGeneration,
}

#[cfg(test)]
impl SessionGenerationSink {
    pub(crate) fn new(inner: Arc<dyn SessionEventSink>, generation: SessionGeneration) -> Self {
        Self { inner, generation }
    }
}

#[cfg(test)]
impl SessionEventSink for SessionGenerationSink {
    fn publish_skill_view(
        &self,
        session_id: &str,
        view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        self.inner
            .publish_skill_view_in(session_id, self.generation, view)
    }

    fn begin_history_replay(&self, session_id: &str) -> HistoryReplayFuture<'_> {
        self.inner
            .begin_history_replay_in(session_id, self.generation)
    }

    /// A dropped publication is not a sink failure.
    fn publish(&self, session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.inner
            .publish_in(session_id, self.generation, event)
            .map(|_| ())
    }

    fn current_generation(&self, _session_id: &str) -> SessionGeneration {
        self.generation
    }

    fn publish_in(
        &self,
        session_id: &str,
        _generation: SessionGeneration,
        event: SessionEvent,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.inner.publish_in(session_id, self.generation, event)
    }

    fn publish_skill_view_in(
        &self,
        session_id: &str,
        _generation: SessionGeneration,
        view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        self.publish_skill_view(session_id, view)
    }

    fn begin_history_replay_in(
        &self,
        session_id: &str,
        _generation: SessionGeneration,
    ) -> HistoryReplayFuture<'_> {
        self.begin_history_replay(session_id)
    }

    fn publish_closed(
        &self,
        session_id: &str,
        _generation: Option<SessionGeneration>,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.inner.publish_closed(session_id, Some(self.generation))
    }

    fn publish_unloaded(
        &self,
        session_id: &str,
        _generation: SessionGeneration,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.inner.publish_unloaded(session_id, self.generation)
    }
}

#[cfg(test)]
#[path = "session_event_sink_test.rs"]
mod tests;
