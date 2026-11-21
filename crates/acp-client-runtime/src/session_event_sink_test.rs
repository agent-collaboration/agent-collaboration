use super::{EventSinkClosed, SessionEventSink};
use session_event_model::SessionEvent;

struct ReplaySink;

impl SessionEventSink for ReplaySink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), crate::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _session_id: &str) -> super::HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(&self, _session_id: &str, _event: SessionEvent) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

/// The replay reset must be awaited before an ACP session/load request.
/// Oracle: specification R13 and program-design AgentSessionClient.attach.
#[tokio::test]
async fn history_replay_contract_is_awaitable() {
    let sink: &dyn SessionEventSink = &ReplaySink;
    assert!(sink.begin_history_replay("fixture-session").await.is_ok());
}

/// Records which generation each call carried.
#[derive(Default)]
struct GenerationRecorder {
    calls: std::sync::Mutex<Vec<(&'static str, Option<super::SessionGeneration>)>>,
}

impl GenerationRecorder {
    fn record(&self, call: &'static str, generation: Option<super::SessionGeneration>) {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((call, generation));
    }
}

impl SessionEventSink for GenerationRecorder {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        self.record("skill view, no generation", None);
        Ok(())
    }

    fn begin_history_replay(&self, _session_id: &str) -> super::HistoryReplayFuture<'_> {
        self.record("replay, no generation", None);
        Box::pin(async { Ok(()) })
    }

    fn publish(&self, _session_id: &str, _event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.record("publish, no generation", None);
        Ok(())
    }

    fn publish_in(
        &self,
        _session_id: &str,
        generation: super::SessionGeneration,
        _event: SessionEvent,
    ) -> Result<super::GenerationPublication, EventSinkClosed> {
        self.record("publish", Some(generation));
        Ok(super::GenerationPublication::Published)
    }

    fn publish_skill_view_in(
        &self,
        _session_id: &str,
        generation: super::SessionGeneration,
        _view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        self.record("skill view", Some(generation));
        Ok(())
    }

    fn begin_history_replay_in(
        &self,
        _session_id: &str,
        generation: super::SessionGeneration,
    ) -> super::HistoryReplayFuture<'_> {
        self.record("replay", Some(generation));
        Box::pin(async { Ok(()) })
    }

    fn publish_closed(
        &self,
        _session_id: &str,
        generation: Option<super::SessionGeneration>,
    ) -> Result<super::GenerationPublication, EventSinkClosed> {
        self.record("closed", generation);
        Ok(super::GenerationPublication::Published)
    }
}

/// Oracle: the Reviewer's G2 and the Lead's rule that every producer
/// publishes with the generation it was admitted under, never the current
/// one. A session's scoped sink (what its actor, projections and replay
/// hold) sends every kind of publication with its own generation, even when
/// the producer calls the plain session-ID methods.
#[tokio::test]
async fn a_session_scoped_sink_publishes_everything_in_its_own_generation() {
    // Arrange
    let recorder = std::sync::Arc::new(GenerationRecorder::default());
    let admitted = super::SessionGeneration::from_number(7);
    let scoped = super::SessionGenerationSink::new(recorder.clone(), admitted);

    // Act
    let published = scoped.publish(
        "session-one",
        SessionEvent::StateChanged {
            state: session_event_model::SessionState::Idle,
        },
    );
    let skill_view =
        scoped.publish_skill_view("session-one", session_event_model::SkillView::Unknown);
    let replay = scoped.begin_history_replay("session-one").await;
    let elsewhere = scoped.publish_in(
        "session-one",
        super::SessionGeneration::from_number(9),
        SessionEvent::StateChanged {
            state: session_event_model::SessionState::Idle,
        },
    );
    let closed = scoped.publish_closed("session-one", None);

    // Assert
    assert!(published.is_ok() && skill_view.is_ok() && replay.is_ok());
    assert!(elsewhere.is_ok() && closed.is_ok());
    let calls = recorder
        .calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        calls,
        vec![
            ("publish", Some(admitted)),
            ("skill view", Some(admitted)),
            ("replay", Some(admitted)),
            ("publish", Some(admitted)),
            ("closed", Some(admitted)),
        ]
    );
}
