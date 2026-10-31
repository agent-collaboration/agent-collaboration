//! Model-neutral prompt content reaches ACP in order and under R11 gates.

#[path = "support/provider_behavior.rs"]
mod test_behavior;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    ExternalProviderRuntimeError, HistoryReplayFuture, InteractionFuture, InteractionPort,
    ProviderPersistenceTarget, RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{ApprovalRequest, InputId, PromptContent, SessionEvent};
use tokio_util::sync::CancellationToken;

const MULTIBLOCK_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-multiblock-agent.ts"
);

#[derive(Default)]
struct NoopInteractionPort {
    approval_requests: AtomicUsize,
}
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
        self.approval_requests.fetch_add(1, Ordering::SeqCst);
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
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(&self, _session_id: &str, _event: SessionEvent) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
async fn fixture_client(
    mode: &str,
) -> (
    tempfile::TempDir,
    AgentSessionClient<NoopInteractionPort>,
    Arc<NoopInteractionPort>,
) {
    let root = tempfile::tempdir().expect("fixture root");
    let port = Arc::new(NoopInteractionPort::default());
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![MULTIBLOCK_FIXTURE.to_owned(), mode.to_owned()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::clone(&port),
        Arc::new(NoopEventSink),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    (root, client, port)
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
fn text_and_link() -> Vec<PromptContent> {
    vec![
        PromptContent::text("Read".to_owned()).expect("text"),
        PromptContent::resource_link(
            "file:///notes.txt".to_owned(),
            "notes.txt".to_owned(),
            Some("text/plain".to_owned()),
        )
        .expect("resource link"),
    ]
}

#[tokio::test]
async fn text_and_resource_link_reach_agent_as_two_ordered_blocks() {
    let (_root, client, _port) = fixture_client("blocks").await;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.submit_with_approval_dispatch_for_input(
            "fixture-session".to_owned(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(text_and_link()),
            ),
            (),
            None,
        ),
    )
    .await
    .expect("prompt completes");
    assert!(result.is_ok(), "prompt result: {result:?}");
    client.shutdown().await;
}

#[tokio::test]
async fn unsupported_image_is_rejected_before_any_acp_prompt() {
    let (_root, client, port) = fixture_client("unsupported").await;
    let image =
        PromptContent::image("image/png".to_owned(), "aGVsbG8=".to_owned(), None).expect("image");
    let (dispatch, dispatch_result) = tokio::sync::oneshot::channel();
    let result = client
        .submit_with_approval_dispatch_for_input(
            "fixture-session".to_owned(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![image]),
            ),
            (),
            Some(dispatch),
        )
        .await;
    assert!(
        matches!(
            result,
            Err(ExternalProviderRuntimeError::UnsupportedContent {
                content_type: "image"
            })
        ),
        "result: {result:?}"
    );
    assert_eq!(
        dispatch_result.await.expect("dispatch result"),
        acp_client_runtime::ProviderPromptDispatchObservation::Refused(
            acp_client_runtime::ProviderDispatchRefusal::UnsupportedContent("image")
        )
    );
    assert_eq!(port.approval_requests.load(Ordering::SeqCst), 0);
    let follow_up = tokio::time::timeout(
        Duration::from_secs(2),
        client.submit_with_approval_dispatch_for_input(
            "fixture-session".to_owned(),
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("After rejection".to_owned())
                        .expect("text prompt"),
                ]),
            ),
            (),
            None,
        ),
    )
    .await
    .expect("follow-up completes");
    assert!(follow_up.is_ok(), "follow-up: {follow_up:?}");
    client.shutdown().await;
}

#[tokio::test]
async fn advertised_image_reaches_agent_with_exact_data_and_mime_type() {
    let (_root, client, _port) = fixture_client("image").await;
    let image =
        PromptContent::image("image/png".to_owned(), "aGVsbG8=".to_owned(), None).expect("image");
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.submit_with_approval_dispatch_for_input(
            "fixture-session".to_owned(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![image]),
            ),
            (),
            None,
        ),
    )
    .await
    .expect("image prompt completes");
    assert!(result.is_ok(), "prompt result: {result:?}");
    client.shutdown().await;
}

#[tokio::test]
async fn multi_block_steer_reaches_extension_in_order() {
    let (_root, client, _port) = fixture_client("steer").await;
    let image =
        PromptContent::image("image/png".to_owned(), "aGVsbG8=".to_owned(), None).expect("image");
    let rejected = client
        .steer_contents_with_input(
            "fixture-session".to_owned(),
            InputId::generate(),
            session_event_model::ProviderSubmission::plain(vec![image]),
        )
        .await;
    assert!(matches!(
        rejected,
        Err(ExternalProviderRuntimeError::UnsupportedContent {
            content_type: "image"
        })
    ));
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.steer_contents_with_input(
            "fixture-session".to_owned(),
            InputId::generate(),
            session_event_model::ProviderSubmission::plain(text_and_link()),
        ),
    )
    .await
    .expect("steer completes");
    assert_eq!(
        result.expect("steer result"),
        acp_client_runtime::ProviderSteeringOutcome::PromptRequired
    );
    client.shutdown().await;
}

#[tokio::test]
async fn multi_block_delivery_retains_approval_context() {
    let (_root, client, port) = fixture_client("approval").await;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.submit_with_approval_dispatch_for_input(
            "fixture-session".to_owned(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(text_and_link()),
            ),
            (),
            None,
        ),
    )
    .await
    .expect("prompt completes");
    assert!(result.is_ok(), "prompt result: {result:?}");
    assert_eq!(port.approval_requests.load(Ordering::SeqCst), 1);
    client.shutdown().await;
}
