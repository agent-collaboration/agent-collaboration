//! Claude's connection auth update is private to the ACP back door.

#[path = "support/provider_behavior.rs"]
mod test_behavior;
use std::{path::PathBuf, sync::Arc};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{ApprovalRequest, ProviderAuthStatus, SessionEvent};
use tokio_util::sync::CancellationToken;

const AUTH_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-auth-agent.ts"
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

struct CaptureEventSink(tokio::sync::mpsc::UnboundedSender<SessionEvent>);
impl SessionEventSink for CaptureEventSink {
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
    fn publish(&self, _session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.0.send(event).map_err(|_| EventSinkClosed)
    }
}

/// Oracle: specification R23 decodes _auth/status_update as connection-scoped
/// authStatus, retaining kind and label only, never account or vendor details.
#[tokio::test]
async fn auth_update_before_first_session_is_shared_without_private_fields() {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![AUTH_FIXTURE.to_owned()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let report = client.capability_report(&session_id).await;
    client.shutdown().await;
    assert_eq!(
        report.auth_status,
        ProviderAuthStatus::Account {
            label: "Connected account".to_owned()
        }
    );
    let diagnostic = format!("{report:?}");
    assert!(!diagnostic.contains("private@example.invalid"));
    assert!(!diagnostic.contains("private"));
}

/// Oracle: specification R23 applies later auth changes to every Session's
/// capability report, without changing Session state by itself.
#[tokio::test]
async fn later_auth_change_publishes_capabilities_and_logged_out_status() {
    let root = tempfile::tempdir().expect("fixture root");
    let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![AUTH_FIXTURE.to_owned()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::new(CaptureEventSink(sender)),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let receive_auth_change = async {
        loop {
            let event = events.recv().await.expect("event sink remains open");
            if let SessionEvent::CapabilitiesChanged { capabilities } = event {
                break capabilities;
            }
        }
    };
    let (prompt_result, capabilities) = tokio::join!(
        client.submit_with_approval_dispatch_for_input(
            session_id.clone(),
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Proceed".to_owned())
                        .expect("text prompt")
                ])
            ),
            (),
            None
        ),
        tokio::time::timeout(std::time::Duration::from_secs(2), receive_auth_change),
    );
    let capabilities = capabilities.expect("auth notification was handled");
    assert_eq!(capabilities.auth_status, ProviderAuthStatus::LoggedOut);
    prompt_result.expect("prompt ends");
    let report = client.capability_report(&session_id).await;
    client.shutdown().await;
    assert_eq!(report.auth_status, ProviderAuthStatus::LoggedOut);
}
