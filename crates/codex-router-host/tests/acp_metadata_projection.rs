//! Real ACP decoding/projection retains metadata without making it Codex prose.
#![expect(
    clippy::expect_used,
    reason = "A malformed fixed fixture must fail its test."
)]

use std::{path::PathBuf, sync::Arc, time::Duration};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    RefusedApprovalOffer, SessionEventSink,
};
use collaboration_runtime::HistoricalTurn;
use collaboration_runtime::render_historical_turn;
use serde_json::{Value, json};
use session_event_model::{
    ApprovalRequest, SessionEvent, SessionItemKind, StopReason, TurnOutcome,
};
use tokio_util::sync::CancellationToken;

const METADATA_PROVIDER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-metadata-agent.ts"
);

struct NoInteractions;

impl InteractionPort for NoInteractions {
    type Context = ();
    type OperationId = u64;

    fn operation_id(_context: &()) -> u64 {
        0
    }
    fn binding_retirement(_context: &()) -> CancellationToken {
        CancellationToken::new()
    }
    fn request_approval(
        &self,
        _context: (),
        _request: ApprovalRequest,
        _turn: CancellationToken,
        _agent: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }
    fn request_question(
        &self,
        _context: (),
        _request: session_event_model::QuestionRequest,
        _turn: CancellationToken,
        _agent: CancellationToken,
    ) -> InteractionFuture<'_, session_event_model::QuestionResponse> {
        Box::pin(async { session_event_model::QuestionResponse::Cancelled })
    }
    fn record_refusal(
        &self,
        _context: (),
        _refusal: RefusedApprovalOffer,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_all(&self, _context: (), _reason: &'static str) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

struct MetadataCapture(tokio::sync::mpsc::UnboundedSender<SessionEvent>);

impl SessionEventSink for MetadataCapture {
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(&self, _session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.0.send(event).map_err(|_| EventSinkClosed)
    }
}

async fn check_metadata_projection(replay: bool) {
    // Arrange: actual ACP subprocess and runtime projection, with literal payloads.
    let root = tempfile::tempdir().expect("provider working directory");
    let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![METADATA_PROVIDER.into()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoInteractions),
        Arc::new(MetadataCapture(sender)),
    )
    .await
    .expect("ACP initializes");

    // Act: both session/load replay and live session/prompt reach the same projection.
    tokio::time::timeout(Duration::from_secs(5), async {
        if replay {
            client
                .load_session("metadata-session".into(), root.path().to_path_buf())
                .await
                .expect("provider replay loads");
        } else {
            let session = client
                .create_session(root.path().to_path_buf())
                .await
                .expect("session opens");
            client
                .prompt_contents_with_approval_dispatch_for_input(
                    session,
                    session_event_model::InputId::generate(),
                    vec![session_event_model::PromptContent::text("Reply".into()).expect("prompt")],
                    (),
                    None,
                )
                .await
                .expect("provider prompt settles");
        }
    })
    .await
    .expect("provider operation deadline");
    client.shutdown().await;
    let mut items = Vec::new();
    while let Ok(event) = received.try_recv() {
        if let SessionEvent::ItemStarted { item } = event {
            items.push(item);
        }
    }

    // Assert retention separately: suppressing the producer would fail these checks.
    let usage = items
        .iter()
        .find(|item| item.kind == SessionItemKind::Usage)
        .expect("usage retained");
    assert_eq!(
        serde_json::from_str::<Value>(usage.text.as_deref().expect("usage body"))
            .expect("usage JSON"),
        json!({"used":1234,"size":200000,"cost":{"amount":0.25,"currency":"USD"},
            "_meta":{"providerDetail":"usage-retained"}})
    );
    let commands = items
        .iter()
        .find(|item| item.kind == SessionItemKind::Notice)
        .expect("command catalogue retained");
    assert_eq!(
        serde_json::from_str::<Value>(commands.text.as_deref().expect("commands body"))
            .expect("commands JSON"),
        json!({"availableCommands":[{"name":"compact","description":"Compact context",
            "input":{"hint":"instructions"}}],"_meta":{"providerDetail":"retained"}})
    );
    assert_eq!(items.len(), 7);
    let assistant_id = items
        .iter()
        .find(|item| item.kind == SessionItemKind::AgentMessage)
        .expect("assistant item")
        .item_id
        .clone();
    let thought_id = items
        .iter()
        .find(|item| item.kind == SessionItemKind::AgentThought)
        .expect("reasoning item")
        .item_id
        .clone();

    // Assert consumer presentation: a raw metadata agentMessage fails this literal oracle.
    let rendered = render_historical_turn(&HistoricalTurn {
        turn_id: "observed-turn".into(),
        items,
        outcome: TurnOutcome::Ended {
            stop_reason: StopReason::EndTurn,
            local_cause: None,
        },
    });
    assert_eq!(
        rendered.get("items"),
        Some(&json!([
            {"type":"agentMessage","id":assistant_id,"text":"Ordinary assistant reply"},
            {"type":"reasoning","id":thought_id,"summary":["Useful reasoning"],"content":[]}
        ]))
    );
}

#[tokio::test]
async fn live_acp_metadata_is_retained_without_assistant_prose() {
    check_metadata_projection(false).await;
}

#[tokio::test]
async fn replayed_acp_metadata_is_retained_without_assistant_prose() {
    check_metadata_projection(true).await;
}
