//! Nonblocking publication boundary for Router-owned Session events.

use std::{future::Future, pin::Pin};

use session_event_model::SessionEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("session event consumer closed")]
pub struct EventSinkClosed;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("session history replay could not begin")]
pub struct HistoryReplayUnavailable;

pub type HistoryReplayFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), HistoryReplayUnavailable>> + Send + 'a>>;

pub trait SessionEventSink: Send + Sync + 'static {
    fn publish_skill_view(
        &self,
        session_id: &str,
        view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed>;
    /// Reset hub history before the agent can publish replayed updates.
    fn begin_history_replay(&self, session_id: &str) -> HistoryReplayFuture<'_>;

    /// Return promptly; failure means the event consumer has closed.
    fn publish(&self, session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed>;
}

#[cfg(test)]
#[path = "session_event_sink_test.rs"]
mod tests;
