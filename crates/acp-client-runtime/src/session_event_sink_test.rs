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
