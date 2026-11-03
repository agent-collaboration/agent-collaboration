//! Cursor's connection requests must retain their originating Session.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{ApprovalRequest, SessionEvent, SessionItemKind};
use tokio_util::sync::CancellationToken;

const TWO_SESSION_TODOS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/cursor-todos-agent.ts"
);

const SINGLE_RUNNING_TODO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/cursor-todos-agent.ts"
);

struct NoopInteractionPort;
impl InteractionPort for NoopInteractionPort {
    type Context = ();
    type OperationId = u64;
    fn operation_id(_context: &Self::Context) -> Self::OperationId {
        1
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

#[derive(Default)]
struct CaptureEventSink(Mutex<Vec<(String, SessionEvent)>>);
impl SessionEventSink for CaptureEventSink {
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    #[expect(
        clippy::unwrap_in_result,
        reason = "This test double must fail on broken fixture invariants instead of reporting a product error."
    )]
    fn publish(&self, session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.0
            .lock()
            .expect("event sink")
            .push((session_id.to_owned(), event));
        Ok(())
    }
}

/// Oracle: Cursor omits sessionId in update_todos; observed tool calls on
/// concurrent Sessions must keep each plan Item in its own Session history.
#[tokio::test]
async fn concurrent_cursor_todos_publish_to_their_observed_sessions() {
    let root = tempfile::tempdir().expect("fixture root");
    let sink = Arc::new(CaptureEventSink::default());
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![TWO_SESSION_TODOS.to_owned(), "two".to_owned()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        sink.clone(),
    )
    .await
    .expect("fixture initializes");
    let first = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("first session");
    let second = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("second session");
    let (first_result, second_result) =
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::join!(
                client.prompt_contents_with_approval_dispatch_for_input(
                    first.clone(),
                    session_event_model::InputId::generate(),
                    vec![
                        session_event_model::PromptContent::text("One".to_owned())
                            .expect("text prompt")
                    ],
                    (),
                    None
                ),
                client.prompt_contents_with_approval_dispatch_for_input(
                    second.clone(),
                    session_event_model::InputId::generate(),
                    vec![
                        session_event_model::PromptContent::text("Two".to_owned())
                            .expect("text prompt")
                    ],
                    (),
                    None
                )
            )
        })
        .await
        .expect("fixture exchanges complete");
    first_result.expect("first prompt");
    second_result.expect("second prompt");
    client.shutdown().await;
    let events = sink.0.lock().expect("event sink");
    for (session_id, expected_text) in [
        (first.as_str(), "First task"),
        (second.as_str(), "Second task"),
    ] {
        assert!(
            events.iter().any(|(owner, event)| owner == session_id
                && matches!(event, SessionEvent::ItemStarted { item }
                if item.kind == SessionItemKind::Plan
                    && item.text.as_deref().is_some_and(|text| text.contains(expected_text)))),
            "plan Item for {session_id}: {events:?}"
        );
        assert!(
            events.iter().any(|(owner, event)| owner == session_id
                && matches!(event, SessionEvent::ItemUpdated { item }
                    if item.kind == SessionItemKind::Plan
                        && item.text.as_deref().is_some_and(|text| text.contains(&format!("Plan for {session_id}"))))),
            "updated plan for {session_id}: {events:?}"
        );
    }
    assert_eq!(events.iter().filter(|(_, event)| matches!(event, SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::Plan)).count(), 2);
}

/// Without a tool card, one running Turn is a unique Session owner.
#[tokio::test]
async fn a_single_running_turn_routes_an_unobserved_cursor_todo() {
    let root = tempfile::tempdir().expect("fixture root");
    let sink = Arc::new(CaptureEventSink::default());
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![SINGLE_RUNNING_TODO.to_owned(), "single".to_owned()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        sink.clone(),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.prompt_contents_with_approval_dispatch_for_input(
            session_id.clone(),
            session_event_model::InputId::generate(),
            vec![session_event_model::PromptContent::text("Go".to_owned()).expect("text prompt")],
            (),
            None,
        ),
    )
    .await
    .expect("exchange completes")
    .expect("prompt ends");
    client.shutdown().await;
    assert!(
        sink.0
            .lock()
            .expect("event sink")
            .iter()
            .any(|(owner, event)| owner == &session_id
                && matches!(event, SessionEvent::ItemStarted { item }
            if item.kind == SessionItemKind::Plan
                && item.text.as_deref().is_some_and(|text| text.contains("Fallback task"))))
    );
}
