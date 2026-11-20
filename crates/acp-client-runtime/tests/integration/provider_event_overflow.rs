//! Output byte limits and event-consumer shutdown have different outcomes.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::acp_owner_fixture::FixtureClient as AgentSessionClient;
use acp_client_runtime::{
    ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch, ExternalProviderRuntimeError,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{ApprovalRequest, LocalCause, SessionEvent, StopReason, TurnOutcome};
use tokio_util::sync::CancellationToken;

const OUTPUT_LIMIT_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-output-limit-agent.ts"
);

const CLOSED_SINK_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-closed-sink-agent.ts"
);

const CLOSED_AUTH_SINK_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-closed-auth-agent.ts"
);

const LOST_PROVIDER_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-lost-agent.ts"
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
struct CaptureEventSink {
    events: Mutex<Vec<SessionEvent>>,
    reject_items: bool,
    reject_capabilities: bool,
    closed: AtomicBool,
}
impl SessionEventSink for CaptureEventSink {
    fn publish_connection_auth(
        &self,
        session: &str,
        auth_status: session_event_model::ProviderAuthStatus,
    ) -> Result<(), EventSinkClosed> {
        self.publish(
            session,
            SessionEvent::CapabilitiesChanged {
                capabilities: session_event_model::CapabilityReport {
                    auth_status,
                    ..Default::default()
                },
            },
        )
    }

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

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    #[expect(
        clippy::unwrap_in_result,
        reason = "This test double must fail on broken fixture invariants instead of reporting a product error."
    )]
    fn publish(&self, _session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(EventSinkClosed);
        }
        if (self.reject_items && matches!(event, SessionEvent::ItemStarted { .. }))
            || (self.reject_capabilities
                && matches!(&event, SessionEvent::CapabilitiesChanged { capabilities }
                    if matches!(capabilities.auth_status, session_event_model::ProviderAuthStatus::LoggedOut)))
        {
            self.closed.store(true, Ordering::SeqCst);
            return Err(EventSinkClosed);
        }
        self.events.lock().expect("event sink").push(event);
        Ok(())
    }
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
async fn run_fixture(
    fixture: &str,
    sink: Arc<CaptureEventSink>,
) -> (
    Result<acp_client_runtime::ExternalProviderPromptOutcome, ExternalProviderRuntimeError>,
    Arc<CaptureEventSink>,
) {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![fixture.to_owned()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::clone(&sink) as Arc<dyn SessionEventSink>,
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let result = client
        .submit_with_approval_dispatch_for_input(
            session_id,
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Run".to_owned())
                        .expect("text prompt"),
                ]),
            ),
            (),
            None,
        )
        .await;
    client.shutdown().await;
    (result, sink)
}

/// R5: a bounded output byte limit cancels the Turn, then keeps the agent's
/// confirmed stop reason alongside Router's separate local cause.
#[tokio::test]
async fn output_byte_limit_cancels_turn_and_keeps_agent_stop_reason() {
    let (result, sink) =
        run_fixture(OUTPUT_LIMIT_FIXTURE, Arc::new(CaptureEventSink::default())).await;
    assert!(matches!(
        result,
        Err(ExternalProviderRuntimeError::PromptOutputLimitExceeded)
    ));
    let events = sink.events.lock().expect("event sink");
    assert!(matches!(
        events.as_slice(),
        [
            SessionEvent::CapabilitiesChanged { .. },
            SessionEvent::SettingsChanged { .. },
            ..
        ]
    ));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnStarted { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        SessionEvent::TurnEnded {
            outcome: TurnOutcome::Ended {
                stop_reason: StopReason::EndTurn,
                local_cause: Some(LocalCause::OutputOverflow),
            },
            ..
        }
    )));
}

/// T6. A failed publication does not replace the provider's prompt result.
#[tokio::test]
async fn closed_event_consumer_keeps_the_provider_prompt_result() {
    let (result, sink) = run_fixture(
        CLOSED_SINK_FIXTURE,
        Arc::new(CaptureEventSink {
            reject_items: true,
            ..CaptureEventSink::default()
        }),
    )
    .await;
    let completed = result.expect("the provider prompt still completes");
    assert_eq!(completed.stop_reason, StopReason::EndTurn);
    assert_eq!(completed.output, "Chunk");
    assert!(
        sink.closed.load(Ordering::SeqCst),
        "the item publication was refused"
    );
    let events = sink.events.lock().expect("event sink");
    assert!(matches!(
        events.as_slice(),
        [
            SessionEvent::CapabilitiesChanged { .. },
            SessionEvent::SettingsChanged { .. },
            ..
        ]
    ));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnStarted { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnEnded { .. }))
    );
}

/// A refused auth publication does not settle the active prompt locally.
#[tokio::test]
async fn closed_auth_event_consumer_keeps_the_provider_prompt_result() {
    let (result, sink) = run_fixture(
        CLOSED_AUTH_SINK_FIXTURE,
        Arc::new(CaptureEventSink {
            reject_capabilities: true,
            ..CaptureEventSink::default()
        }),
    )
    .await;
    assert_eq!(
        result
            .expect("the provider prompt still completes")
            .stop_reason,
        StopReason::EndTurn
    );
    assert!(
        sink.closed.load(Ordering::SeqCst),
        "the signed-out publication was refused"
    );
    let events = sink.events.lock().expect("event sink");
    assert!(matches!(
        events.as_slice(),
        [
            SessionEvent::CapabilitiesChanged { .. },
            SessionEvent::SettingsChanged { .. },
            ..
        ]
    ));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnStarted { .. }))
    );
}

/// A provider that closes stdout before its prompt result cannot confirm a
/// stop reason; the Turn is lost with the retirement reason.
#[tokio::test]
async fn provider_eof_ends_running_turn_lost() {
    let (result, sink) =
        run_fixture(LOST_PROVIDER_FIXTURE, Arc::new(CaptureEventSink::default())).await;
    assert!(result.is_err());
    let events = sink.events.lock().expect("event sink");
    assert!(matches!(
        events.as_slice(),
        [
            SessionEvent::CapabilitiesChanged { .. },
            SessionEvent::SettingsChanged { .. },
            ..
        ]
    ));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnStarted { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnEnded {
        outcome: TurnOutcome::Lost { reason }, ..
    } if reason == &session_event_model::TurnLostReason::ProviderRetired))
    );
    assert!(matches!(
        events.last(),
        Some(SessionEvent::StateChanged {
            state: session_event_model::SessionState::Unloaded
        })
    ));
}
