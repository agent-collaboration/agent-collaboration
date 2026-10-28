//! An unknown ACP stop reason still ends the Turn with its output intact.

use std::{path::PathBuf, sync::Arc};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{ApprovalRequest, SessionEvent, StopReason};
use tokio_util::sync::CancellationToken;

const UNKNOWN_STOP_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/unknown-stop-agent.ts"
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

struct NoopEventSink;
impl SessionEventSink for NoopEventSink {
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(&self, _session_id: &str, _event: SessionEvent) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

/// Oracle: specification R4 says unknown(value) ends the Turn and preserves
/// output already produced; ACP v1 tool-calls.mdx lists the five known reasons.
#[tokio::test]
async fn unknown_stop_reason_preserves_both_output_chunks() {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![UNKNOWN_STOP_FIXTURE.to_owned()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let outcome = client
        .prompt_contents_with_approval_dispatch_for_input(
            session_id,
            session_event_model::InputId::generate(),
            vec![
                session_event_model::PromptContent::text("Proceed".to_owned())
                    .expect("text prompt"),
            ],
            (),
            None,
        )
        .await
        .expect("prompt ends");
    client.shutdown().await;
    assert_eq!(outcome.output, "first second");
    assert_eq!(
        outcome.stop_reason,
        StopReason::Unknown("future_reason".to_owned())
    );
}
