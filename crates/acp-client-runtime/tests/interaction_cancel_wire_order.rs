//! ACP interaction replies must enter the wire before a Turn cancellation.

#![allow(clippy::expect_used)]

use std::{path::PathBuf, sync::Arc, time::Duration};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    ProviderPromptDispatchObservation, RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{ApprovalRequest, QuestionRequest, QuestionResponse, SessionEvent};
use tokio_util::sync::CancellationToken;

const INTERACTION_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/interaction-cancel-order-agent.ts"
);

struct HeldInteractionPort {
    started: Arc<tokio::sync::Notify>,
}

impl InteractionPort for HeldInteractionPort {
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
        turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        let started = Arc::clone(&self.started);
        Box::pin(async move {
            started.notify_one();
            turn_cancellation.cancelled().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            ApprovalPortOutcome::Cancelled
        })
    }

    fn request_question(
        &self,
        _context: Self::Context,
        _request: QuestionRequest,
        turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, QuestionResponse> {
        let started = Arc::clone(&self.started);
        Box::pin(async move {
            started.notify_one();
            turn_cancellation.cancelled().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            QuestionResponse::Cancelled
        })
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

struct NoopEventSink;

impl SessionEventSink for NoopEventSink {
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(&self, _session_id: &str, _event: SessionEvent) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

async fn assert_cancelled_reply_precedes_cancel(kind: &str) {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire-order.json");
    let started = Arc::new(tokio::sync::Notify::new());
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                INTERACTION_FIXTURE.to_owned(),
                kind.to_owned(),
                receipt.to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(HeldInteractionPort {
            started: Arc::clone(&started),
        }),
        Arc::new(NoopEventSink),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let (dispatch_tx, dispatch_rx) = tokio::sync::oneshot::channel();
    let prompt = client.prompt_contents_with_approval_dispatch_for_input(
        session_id.clone(),
        session_event_model::InputId::generate(),
        vec![session_event_model::PromptContent::text("Interact".to_owned()).expect("text prompt")],
        (),
        Some(dispatch_tx),
    );
    tokio::pin!(prompt);
    let dispatch = tokio::select! {
        receipt = dispatch_rx => receipt.expect("dispatch receipt"),
        result = &mut prompt => panic!("prompt settled before dispatch: {result:?}"),
    };
    assert_eq!(dispatch, ProviderPromptDispatchObservation::Submitted);
    tokio::time::timeout(Duration::from_secs(2), started.notified())
        .await
        .expect("interaction reached port");
    tokio::time::timeout(
        Duration::from_secs(2),
        client.cancel_active_prompt(session_id),
    )
    .await
    .expect("cancel deadline")
    .expect("cancel notification");
    tokio::time::timeout(Duration::from_secs(2), &mut prompt)
        .await
        .expect("prompt settlement deadline")
        .expect("prompt settles");
    client.shutdown().await;

    let wire: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(receipt).expect("wire receipt")).expect("wire JSON");
    let first = wire.first().expect("interaction reply frame");
    let second = wire.get(1).expect("cancel notification frame");
    assert_eq!(
        first.get("id"),
        Some(&serde_json::json!("interaction")),
        "{kind} first wire frame: {wire:?}"
    );
    let expected_reply = match kind {
        "form" => serde_json::json!({"action":"cancel"}),
        "question" => serde_json::json!({"outcome":{"outcome":"cancelled"}}),
        "plan" => serde_json::json!({"outcome":"cancelled"}),
        _ => serde_json::Value::Null,
    };
    assert_eq!(first.get("result"), Some(&expected_reply));
    assert_eq!(
        second.get("method"),
        Some(&serde_json::json!("session/cancel")),
        "{kind} second wire frame: {wire:?}"
    );
}

#[tokio::test]
async fn form_elicitation_reply_precedes_session_cancel() {
    assert_cancelled_reply_precedes_cancel("form").await;
}

#[tokio::test]
async fn cursor_question_reply_precedes_session_cancel() {
    assert_cancelled_reply_precedes_cancel("question").await;
}

#[tokio::test]
async fn cursor_plan_reply_precedes_session_cancel() {
    assert_cancelled_reply_precedes_cancel("plan").await;
}
