//! ACP interaction replies must enter the wire before a Turn cancellation.

use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::acp_owner_fixture::FixtureClient as AgentSessionClient;
use acp_client_runtime::{
    ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch, HistoryReplayFuture,
    InteractionFuture, InteractionPort, ProviderModelPicker, ProviderPersistenceTarget,
    ProviderPromptDispatchObservation, RefusedApprovalOffer, SessionEventSink,
};
use provider_session_machines::session_owner::{CancelDrainReport, PendingTurnResponse};
use router_clock::{ClockInstant, ManualClock};
use session_event_model::{ApprovalRequest, QuestionRequest, QuestionResponse, SessionEvent};
use tokio_util::sync::CancellationToken;

const INTERACTION_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/interaction-cancel-order-agent.ts"
);

struct HeldInteractionPort {
    started: Arc<tokio::sync::Notify>,
    cancellation_observed: Arc<tokio::sync::Notify>,
    permit: Arc<tokio::sync::Semaphore>,
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

    #[expect(
        clippy::expect_used,
        reason = "A closed fixture semaphore invalidates the wire-order test."
    )]
    fn request_approval(
        &self,
        _context: Self::Context,
        _request: ApprovalRequest,
        turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        let started = Arc::clone(&self.started);
        let cancellation_observed = self.cancellation_observed.clone();
        let permit = self.permit.clone();
        Box::pin(async move {
            started.notify_one();
            turn_cancellation.cancelled().await;
            cancellation_observed.notify_one();
            permit.acquire().await.expect("reply permit").forget();
            ApprovalPortOutcome::Cancelled
        })
    }

    #[expect(
        clippy::expect_used,
        reason = "A closed fixture semaphore invalidates the wire-order test."
    )]
    fn request_question(
        &self,
        _context: Self::Context,
        _request: QuestionRequest,
        turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, QuestionResponse> {
        let started = Arc::clone(&self.started);
        let cancellation_observed = self.cancellation_observed.clone();
        let permit = self.permit.clone();
        Box::pin(async move {
            started.notify_one();
            turn_cancellation.cancelled().await;
            cancellation_observed.notify_one();
            permit.acquire().await.expect("reply permit").forget();
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
async fn assert_cancel_order(kind: &str, expired: bool) {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire-order.json");
    let started = Arc::new(tokio::sync::Notify::new());
    let cancellation_observed = Arc::new(tokio::sync::Notify::new());
    let permit = Arc::new(tokio::sync::Semaphore::new(0));
    let clock = Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH));
    let client = AgentSessionClient::initialize_with_mcp_http(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                INTERACTION_FIXTURE.to_owned(),
                kind.to_owned(),
                receipt.to_string_lossy().into_owned(),
                if expired { "expire" } else { "drain" }.to_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        Arc::new(HeldInteractionPort {
            started: Arc::clone(&started),
            cancellation_observed: cancellation_observed.clone(),
            permit: permit.clone(),
        }),
        Arc::new(NoopEventSink),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
        clock.clone(),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let independent = if expired {
        Some(
            client
                .create_session(root.path().to_path_buf())
                .await
                .expect("independent session"),
        )
    } else {
        None
    };
    let (dispatch_tx, dispatch_rx) = tokio::sync::oneshot::channel();
    let prompt = client.submit_with_approval_dispatch_for_input(
        session_id.clone(),
        session_event_model::InputId::generate(),
        acp_client_runtime::ProviderTurnInput::Prompt(
            session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::text("Interact".to_owned())
                    .expect("text prompt"),
            ]),
        ),
        (),
        Some(dispatch_tx),
    );
    tokio::pin!(prompt);
    let dispatch = tokio::select! {
        receipt = dispatch_rx => receipt.expect("dispatch receipt"),
        result = &mut prompt => panic!("prompt settled before dispatch: {result:?}"),
    };
    assert_eq!(dispatch, ProviderPromptDispatchObservation::Submitted);
    started.notified().await;
    let delay = Duration::from_secs(10);
    let mut cancel = Box::pin(client.cancel_with_report_fixture(session_id));
    tokio::select! {
        () = cancellation_observed.notified() => {}
        result = &mut cancel => panic!("cancel was sent before its scoped response could drain: {result:?}"),
    }
    tokio::select! {
        () = clock.not_due_check_now(ClockInstant::ZERO.after(delay)) => {}
        result = &mut cancel => panic!("cancel escaped the held interaction: {result:?}"),
    }
    if let Some(independent) = independent {
        client
            .submit_with_approval_dispatch_for_input(
                independent,
                session_event_model::InputId::generate(),
                acp_client_runtime::ProviderTurnInput::Prompt(
                    session_event_model::ProviderSubmission::plain(vec![
                        session_event_model::PromptContent::text("Independent".to_owned())
                            .expect("prompt"),
                    ]),
                ),
                (),
                None,
            )
            .await
            .expect("the other session completes while the drain is held");
        clock.advance(delay).expect("reach the drain deadline");
        let sent = cancel.await.expect("cancel was sent at L");
        assert_eq!(
            sent,
            CancelDrainReport::Expired {
                pending: vec![PendingTurnResponse {
                    method: "elicitation/create",
                    request_id: "\"interaction\"".to_owned(),
                }],
            }
        );
        permit.add_permits(1);
    } else {
        permit.add_permits(1);
        assert_eq!(
            cancel.await.expect("cancel was sent after draining"),
            CancelDrainReport::Drained
        );
    }
    prompt.await.expect("prompt settles");
    client.shutdown().await;

    let wire: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(receipt).expect("wire receipt")).expect("wire JSON");
    let first = wire
        .get(usize::from(expired))
        .expect("interaction reply frame");
    let second = wire
        .get(usize::from(!expired))
        .expect("cancel notification frame");
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
    assert_cancel_order("form", false).await;
}

#[tokio::test]
async fn cursor_question_reply_precedes_session_cancel() {
    assert_cancel_order("question", false).await;
}

#[tokio::test]
async fn cursor_plan_reply_precedes_session_cancel() {
    assert_cancel_order("plan", false).await;
}

#[tokio::test]
async fn expired_drain_sends_cancel_and_reports_the_pending_reply() {
    assert_cancel_order("form", true).await;
}
