//! A start whose mode answer is lost stays held until the mode is set again
//! and confirmed (D1 spec R2 "Its answer is lost or unreadable", Held).

#![expect(
    clippy::expect_used,
    reason = "A fixture that fails to start or answer must fail the test at once."
)]

use crate::acp_owner_fixture::FixtureClient as AgentSessionClient;
use acp_client_runtime::{
    ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch, ExternalProviderRuntimeError,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    ProviderSettingRequest, RefusedApprovalOffer, RequestedProviderSettings, SessionEventSink,
};
use provider_session_machines::ProviderSettingKind;
use serde_json::{Value, json};
use session_event_model::{ApprovalRequest, InputId, SessionEvent};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;

const PERMISSION_MODE_AGENT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/permission-mode-agent.ts"
);

/// Counts every approval that reaches the Router's approval path.
#[derive(Default)]
struct CountingApprovals {
    asked: AtomicUsize,
}

impl InteractionPort for CountingApprovals {
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
        _context: (),
        _request: ApprovalRequest,
        _turn: CancellationToken,
        _agent: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        self.asked.fetch_add(1, Ordering::SeqCst);
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
}

struct IgnoredEvents;
impl SessionEventSink for IgnoredEvents {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(&self, _session_id: &str, _event: SessionEvent) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

async fn prompt_in_folder_edit(
    client: &AgentSessionClient<CountingApprovals>,
    session: &str,
    scratch: &std::path::Path,
) -> Result<(), ExternalProviderRuntimeError> {
    let edit = json!({"kind":"edit","locations":[{"path":scratch.join("note.txt")}]});
    client
        .submit_with_approval_dispatch_for_input(
            session.to_owned(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text(edit.to_string())
                        .expect("text prompt"),
                ]),
            ),
            (),
            None,
        )
        .await
        .map(|_| ())
}

/// Oracle: D1 verification V6 steps 1, 4, 5, 6 and 7 — a lost auto answer is
/// "created without settings" with outcome unknown; prompts are refused and
/// never reach the agent; accepting is refused; a confirmed auto set releases
/// the hold, and the next in-folder edit runs without an approval.
#[tokio::test]
async fn lost_auto_answer_is_unconfirmed_held_and_has_no_allowance() {
    let root = tempfile::tempdir().expect("fixture root");
    let base = std::fs::canonicalize(root.path()).expect("canonical root");
    let (project, scratch) = (base.join("project"), base.join("scratch/session-a"));
    std::fs::create_dir_all(&project).expect("project");
    std::fs::create_dir_all(&scratch).expect("scratch");
    let receipt = base.join("receipt.json");
    let approvals = Arc::new(CountingApprovals::default());
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                PERMISSION_MODE_AGENT.to_owned(),
                receipt.display().to_string(),
                "claude".to_owned(),
                "grants-directories".to_owned(),
                "malformed-mode-reply".to_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::clone(&approvals),
        Arc::new(IgnoredEvents),
        Arc::new(crate::test_behavior::ClaudeLikeAutoBehavior),
    )
    .await
    .expect("fixture initializes");

    let created = client
        .create_session_with_settings(
            project,
            Some(scratch.clone()),
            RequestedProviderSettings {
                mode: Some("auto".to_owned()),
                ..RequestedProviderSettings::default()
            },
        )
        .await;
    let Err(ExternalProviderRuntimeError::CreatedWithoutSettings {
        provider_session_id: session,
        failed,
        ..
    }) = created
    else {
        panic!("expected a start created without settings, got {created:?}");
    };
    let first_prompt = prompt_in_folder_edit(&client, &session, &scratch).await;
    let accepted = client.accept_session_settings(session.clone()).await;
    let second_prompt = prompt_in_folder_edit(&client, &session, &scratch).await;
    let read_before_set: Value =
        serde_json::from_slice(&std::fs::read(&receipt).expect("receipt")).expect("JSON");
    client
        .set_setting(
            session.clone(),
            ProviderSettingRequest {
                kind: ProviderSettingKind::Mode,
                value: "auto".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
        .expect("confirmed auto");
    let released = prompt_in_folder_edit(&client, &session, &scratch).await;
    client.shutdown().await;
    let read_after: Value =
        serde_json::from_slice(&std::fs::read(&receipt).expect("receipt")).expect("JSON");

    assert_eq!(failed.kind, ProviderSettingKind::Mode);
    assert_eq!(
        failed.reason,
        session_event_model::ProviderSettingFailureReason::OutcomeUnknown
    );
    assert!(
        matches!(
            first_prompt,
            Err(ExternalProviderRuntimeError::SettingsUnresolved { .. })
        ),
        "{first_prompt:?}"
    );
    assert!(
        matches!(
            accepted,
            Err(ExternalProviderRuntimeError::SettingsOutcomeUncertain)
        ),
        "{accepted:?}"
    );
    assert!(
        matches!(
            second_prompt,
            Err(ExternalProviderRuntimeError::SettingsUnresolved { .. })
        ),
        "{second_prompt:?}"
    );
    assert_eq!(
        read_before_set["prompts"],
        json!([]),
        "no held prompt reached the agent"
    );
    released.expect("the released prompt runs");
    assert_eq!(
        read_after["prompts"],
        json!([session]),
        "exactly one prompt reached the agent"
    );
    assert_eq!(
        approvals.asked.load(Ordering::SeqCst),
        0,
        "the in-folder edit ran without an approval"
    );
}
