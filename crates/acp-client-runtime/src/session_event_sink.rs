//! Nonblocking publication boundary for Router-owned Session events.

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

/// One run of a session, from the create, load or resume the connection
/// admitted to its Closed (D1 R14). Every per-session publication carries the
/// generation it was produced under; once Closed is published for a
/// generation, a sink that tracks generations publishes nothing more for it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SessionGeneration(u64);

impl SessionGeneration {
    /// What a sink that doesn't track generations hands out.
    pub const UNTRACKED: Self = Self(0);

    /// A tracking sink's numbering.
    pub const fn from_number(number: u64) -> Self {
        Self(number)
    }

    pub const fn number(self) -> u64 {
        self.0
    }
}

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

    /// A new generation for a session the connection has just admitted.
    fn open_generation(&self, _session_id: &str) -> SessionGeneration {
        SessionGeneration::UNTRACKED
    }

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
}

/// One admitted session's view of the sink: every publication carries the
/// generation the session was admitted under, so a producer that outlives
/// its run (an actor, a projection, a settings update) can't publish into a
/// later one.
pub(crate) struct SessionGenerationSink {
    inner: Arc<dyn SessionEventSink>,
    generation: SessionGeneration,
}

impl SessionGenerationSink {
    pub(crate) fn new(inner: Arc<dyn SessionEventSink>, generation: SessionGeneration) -> Self {
        Self { inner, generation }
    }
}

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

    fn open_generation(&self, session_id: &str) -> SessionGeneration {
        self.inner.open_generation(session_id)
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
}

#[cfg(test)]
#[path = "session_event_sink_test.rs"]
mod tests;
