//! A subprocess checks the ACP wire order around history replay admission.

#[path = "support/provider_behavior.rs"]
mod test_behavior;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    ExternalProviderRuntimeError, HistoryReplayFuture, HistoryReplayUnavailable, InteractionFuture,
    InteractionPort, RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{ApprovalRequest, SessionEvent};
use tokio_util::sync::CancellationToken;

const REPLAY_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-history-agent.ts"
);

const REPLAY_ITEMS_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-history-agent.ts"
);

const REPLAY_LIVE_TOOL_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-history-agent.ts"
);

struct TestInteractionPort;

impl InteractionPort for TestInteractionPort {
    type Context = ();
    type OperationId = u64;

    fn operation_id(_context: &Self::Context) -> Self::OperationId {
        0
    }

    fn binding_retirement(_context: &Self::Context) -> CancellationToken {
        CancellationToken::new()
    }

    fn request_approval(
        &self,
        _context: Self::Context,
        _request: ApprovalRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }

    fn request_question(
        &self,
        _context: Self::Context,
        _request: session_event_model::QuestionRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, session_event_model::QuestionResponse> {
        Box::pin(async { session_event_model::QuestionResponse::Cancelled })
    }
    fn record_refusal(
        &self,
        _context: Self::Context,
        _refusal: RefusedApprovalOffer,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }

    fn cancel_all(
        &self,
        _context: Self::Context,
        _reason: &'static str,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }

    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

struct ReplaySink {
    marker: PathBuf,
    fail: bool,
    events: Option<Arc<Mutex<Vec<SessionEvent>>>>,
}

impl SessionEventSink for ReplaySink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async move {
            if self.fail {
                return Err(HistoryReplayUnavailable);
            }
            std::fs::write(&self.marker, b"replay reset").map_err(|_| HistoryReplayUnavailable)
        })
    }

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    #[expect(
        clippy::unwrap_in_result,
        reason = "This test double must fail on broken fixture invariants instead of reporting a product error."
    )]
    fn publish(&self, _session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        if let Some(events) = &self.events {
            events.lock().expect("replay events").push(event);
        }
        Ok(())
    }
}

fn fixture_launch(marker: &std::path::Path) -> ExternalProviderLaunch {
    ExternalProviderLaunch {
        executable: PathBuf::from("node"),
        arguments: vec![
            REPLAY_FIXTURE.to_owned(),
            "reset".to_owned(),
            marker.to_string_lossy().into_owned(),
        ],
        environment: Vec::new(),
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
    }
}

/// A live update after load addresses the Item established by replay, rather
/// than starting a second Item with the same provider tool-call ID.
#[tokio::test]
async fn live_update_continues_replayed_item_projection() {
    let root = tempfile::tempdir().expect("fixture root");
    let marker = root.path().join("reset-finished");
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![REPLAY_LIVE_TOOL_FIXTURE.to_owned(), "live".to_owned()],
            environment: Vec::new(),
            persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(TestInteractionPort),
        Arc::new(ReplaySink {
            marker,
            fail: false,
            events: Some(Arc::clone(&events)),
        }),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    client
        .load_session("fixture-session".to_owned(), root.path().to_path_buf())
        .await
        .expect("history loads");
    let catalog = client
        .settings_catalog("fixture-session")
        .await
        .expect("loaded catalog");
    assert_eq!(catalog.effective_settings().mode.as_deref(), Some("ask"));
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.submit_with_approval_dispatch_for_input(
            "fixture-session".to_owned(),
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Continue".to_owned())
                        .expect("text prompt"),
                ]),
            ),
            (),
            None,
        ),
    )
    .await
    .expect("prompt settles")
    .expect("prompt succeeds");
    client.shutdown().await;
    let events = events.lock().expect("events");
    let relevant = events
        .iter()
        .filter(|event| match event {
            SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item } => {
                item.item_id == "replayed-tool"
            }
            _ => false,
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(
            relevant.as_slice(),
            [
                SessionEvent::ItemStarted { .. },
                SessionEvent::ItemUpdated { .. }
            ]
        ),
        "replayed tool events: {relevant:?}"
    );
}

/// Oracle: ACP v1 session/load replays updates before its response
/// (docs/protocol/v1/session-setup.mdx:150-211); program design requires the
/// hub reset to finish before the request can be sent.
#[tokio::test]
async fn history_replay_reset_precedes_session_load() {
    let root = tempfile::tempdir().expect("fixture root");
    let marker = root.path().join("reset-finished");
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = AgentSessionClient::initialize(
        fixture_launch(&marker),
        Arc::new(TestInteractionPort),
        Arc::new(ReplaySink {
            marker: marker.clone(),
            fail: false,
            events: Some(Arc::clone(&events)),
        }),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");

    let result = client
        .load_session("fixture-session".to_owned(), root.path().to_path_buf())
        .await;
    let catalog = client
        .settings_catalog("fixture-session")
        .await
        .expect("loaded session catalog");
    client.shutdown().await;

    assert!(result.is_ok(), "load result: {result:?}");
    assert!(marker.exists(), "reset completed");
    assert_eq!(
        catalog.effective_settings().model.as_deref(),
        Some("model-a")
    );
    let published = events.lock().expect("events");
    assert!(published.iter().any(|event| matches!(event,
        SessionEvent::SettingsChanged { settings }
            if settings.model.as_deref() == Some("model-a"))));
    assert!(matches!(
        published.last(),
        Some(SessionEvent::StateChanged {
            state: session_event_model::SessionState::Unloaded,
        })
    ));
    assert!(
        !marker.with_extension("early").exists(),
        "load waited for reset"
    );
}

/// Oracle: program design AgentSessionClient.attach requires a failed hub reset
/// to abort the load, so the agent cannot replay into stale history.
#[tokio::test]
async fn failed_history_replay_never_sends_session_load() {
    let root = tempfile::tempdir().expect("fixture root");
    let marker = root.path().join("reset-finished");
    let client = AgentSessionClient::initialize(
        fixture_launch(&marker),
        Arc::new(TestInteractionPort),
        Arc::new(ReplaySink {
            marker: marker.clone(),
            fail: true,
            events: None,
        }),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");

    let result = client
        .load_session("fixture-session".to_owned(), root.path().to_path_buf())
        .await;
    client.shutdown().await;

    assert!(matches!(
        result,
        Err(ExternalProviderRuntimeError::HistoryReplayUnavailable)
    ));
    assert!(!marker.with_extension("early").exists(), "no ACP load sent");
}

/// Oracle: E4/R25 rebuild one historical Turn per replayed user message,
/// with unknown(replayed) instead of inventing an original stop reason.
#[tokio::test]
async fn session_load_publishes_two_historical_turns_and_items() {
    let root = tempfile::tempdir().expect("fixture root");
    let marker = root.path().join("reset-finished");
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![REPLAY_ITEMS_FIXTURE.to_owned(), "items".to_owned()],
            environment: Vec::new(),
            persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(TestInteractionPort),
        Arc::new(ReplaySink {
            marker,
            fail: false,
            events: Some(Arc::clone(&events)),
        }),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    client
        .load_session("fixture-session".to_owned(), root.path().to_path_buf())
        .await
        .expect("load");
    client.shutdown().await;
    let events = events.lock().expect("replay events");
    let starts = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::TurnStarted { turn_id, input_id } => Some((turn_id, input_id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2, "replayed events: {events:?}");
    assert_ne!(starts[0].0, starts[1].0);
    assert_ne!(starts[0].1, starts[1].1);
    let ends = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::TurnEnded { turn_id, outcome } => Some((turn_id, outcome)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ends.len(), 2, "replayed events: {events:?}");
    for ((started_id, _), (ended_id, outcome)) in starts.iter().zip(ends) {
        assert_eq!(*started_id, ended_id);
        assert_eq!(
            *outcome,
            session_event_model::TurnOutcome::Ended {
                stop_reason: session_event_model::StopReason::Unknown("replayed".to_owned()),
                local_cause: None,
            }
        );
    }
    let texts = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::ItemStarted { item } => item.text.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        ["First user", "First answer", "Second user", "Second answer"]
    );
}
