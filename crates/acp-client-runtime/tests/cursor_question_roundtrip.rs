//! Cursor questions cross the real ACP dispatcher and the typed Question port.

#[path = "support/provider_behavior.rs"]
mod test_behavior;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{
    ApprovalRequest, ChoiceOption, QuestionAnswerValue, QuestionField, QuestionRequest,
    QuestionResponse, SessionEvent,
};
use tokio_util::sync::CancellationToken;

const ASK_QUESTION_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/cursor-question-agent.ts"
);

struct QuestionPort {
    response: QuestionResponse,
    captured: Mutex<Option<QuestionRequest>>,
}

impl InteractionPort for QuestionPort {
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

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    fn request_question(
        &self,
        _context: Self::Context,
        request: QuestionRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, QuestionResponse> {
        *self.captured.lock().expect("question capture") = Some(request);
        let response = self.response.clone();
        Box::pin(async move { response })
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

/// Oracle: R18 preserves distinct option IDs even when their labels match,
/// and sends both selected IDs for a Cursor multi-select question.
#[tokio::test]
async fn cursor_question_preserves_choice_ids_and_answer_shape() {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("question-response.json");
    let mut content = BTreeMap::new();
    content.insert(
        "single".to_owned(),
        QuestionAnswerValue::SelectedOptions {
            selected_option_ids: vec!["second-id".to_owned()],
        },
    );
    content.insert(
        "multi".to_owned(),
        QuestionAnswerValue::SelectedOptions {
            selected_option_ids: vec!["alpha".to_owned(), "beta".to_owned()],
        },
    );
    let port = Arc::new(QuestionPort {
        response: QuestionResponse::Answered { content },
        captured: Mutex::new(None),
    });
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                ASK_QUESTION_FIXTURE.to_owned(),
                receipt.to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        port.clone(),
        Arc::new(NoopEventSink),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.submit_with_approval_dispatch_for_input(
            session_id,
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Ask".to_owned())
                        .expect("text prompt"),
                ]),
            ),
            (),
            None,
        ),
    )
    .await
    .expect("question exchange completes")
    .expect("prompt ends");
    client.shutdown().await;
    let response: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt).expect("receipt")).expect("response JSON");
    assert_eq!(
        response["result"],
        serde_json::json!({"outcome":{"outcome":"answered","answers":[
        {"questionId":"single","selectedOptionIds":["second-id"]},
        {"questionId":"multi","selectedOptionIds":["alpha","beta"]}]}})
    );
    let question = port
        .captured
        .lock()
        .expect("question capture")
        .clone()
        .expect("question routed");
    let fields = question.fields.iter().cloned().collect::<Vec<_>>();
    assert!(
        matches!(&fields[0], QuestionField::SingleChoice { options, .. }
        if options == &vec![
            ChoiceOption { option_id: "first-id".to_owned(), label: "Same".to_owned() },
            ChoiceOption { option_id: "second-id".to_owned(), label: "Same".to_owned() },
        ])
    );
    assert!(matches!(&fields[1], QuestionField::MultiChoice { options, .. } if options.len() == 2));
}
