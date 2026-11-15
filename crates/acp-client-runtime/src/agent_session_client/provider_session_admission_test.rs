//! Which in-folder grant a start leaves behind, read from the grant map the
//! permission path uses (D1 spec R2, R4).

#![expect(
    clippy::expect_used,
    reason = "A fixture that fails to start or answer must fail the test at once."
)]

use crate::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    ExternalProviderRuntimeError, HistoryReplayFuture, InteractionFuture, InteractionPort,
    InvalidSettingSessionDisposition, ProviderPersistenceTarget, ProviderSettingKind,
    ProviderSettingRequest, RefusedApprovalOffer, RequestedProviderSettings, SessionEventSink,
};
use serde_json::{Value, json};
use session_event_model::{ApprovalRequest, InputId, SessionEvent};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const PERMISSION_MODE_AGENT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/permission-mode-agent.ts"
);

struct CancellingApprovals;
impl InteractionPort for CancellingApprovals {
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

struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    scratch: PathBuf,
    receipt: PathBuf,
    client: AgentSessionClient<CancellingApprovals>,
}

impl Fixture {
    async fn start(behaviours: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let base = std::fs::canonicalize(root.path()).expect("canonical root");
        let project = base.join("project");
        let scratch = base.join("scratch/session-a");
        std::fs::create_dir_all(&project).expect("project");
        std::fs::create_dir_all(&scratch).expect("scratch");
        let receipt = base.join("receipt.json");
        let mut arguments = vec![
            PERMISSION_MODE_AGENT.to_owned(),
            receipt.display().to_string(),
            "claude".to_owned(),
            "grants-directories".to_owned(),
        ];
        arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
        let client = AgentSessionClient::initialize(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments,
                environment: Vec::new(),
                persistence_target: ProviderPersistenceTarget::Unspecified,
            },
            Arc::new(CancellingApprovals),
            Arc::new(IgnoredEvents),
            Arc::new(crate::test_behavior::ClaudeLikeAutoBehavior),
        )
        .await
        .expect("fixture initializes");
        Self {
            _root: root,
            project,
            scratch,
            receipt,
            client,
        }
    }

    async fn create(
        &self,
        mode: &str,
    ) -> Result<crate::ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
        self.client
            .create_session_with_settings(
                self.project.clone(),
                Some(self.scratch.clone()),
                RequestedProviderSettings {
                    mode: Some(mode.to_owned()),
                    ..RequestedProviderSettings::default()
                },
            )
            .await
    }

    async fn prompt_in_folder_edit(
        &self,
        session: &str,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let edit = json!({"kind":"edit","locations":[{"path":self.scratch.join("note.txt")}]});
        self.client
            .submit_with_approval_dispatch_for_input(
                session.to_owned(),
                InputId::generate(),
                crate::ProviderTurnInput::Prompt(session_event_model::ProviderSubmission::plain(
                    vec![
                        session_event_model::PromptContent::text(edit.to_string())
                            .expect("text prompt"),
                    ],
                )),
                (),
                None,
            )
            .await
            .map(|_| ())
    }

    fn receipt(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.receipt).expect("receipt")).expect("JSON")
    }
}

fn created_without_settings_session(
    result: Result<crate::ExternalProviderCreatedSession, ExternalProviderRuntimeError>,
) -> (String, crate::FailedProviderSetting) {
    match result {
        Err(ExternalProviderRuntimeError::CreatedWithoutSettings {
            provider_session_id,
            failed,
            ..
        }) => (provider_session_id, failed),
        other => panic!("expected a start created without settings, got {other:?}"),
    }
}

fn auto_mode() -> ProviderSettingRequest {
    ProviderSettingRequest {
        kind: ProviderSettingKind::Mode,
        value: "auto".to_owned(),
        intent: session_event_model::SettingApplicationIntent::ExplicitChange,
        expected_model: None,
    }
}

/// Oracle: D1 spec R2 row "Refuses the change" and R4 — created without
/// settings, failed setting mode, the agent's known starting mode stays,
/// prompts are held, and the in-folder allowance is off.
#[tokio::test]
async fn refused_auto_is_created_without_settings_held_and_has_no_allowance() {
    let fixture = Fixture::start(&["reject-mode"]).await;

    let (session, failed) = created_without_settings_session(fixture.create("auto").await);
    let held = fixture.prompt_in_folder_edit(&session).await;
    let current_mode = fixture
        .client
        .settings_catalog(&session)
        .await
        .expect("catalog")
        .effective_settings()
        .mode;
    let router_auto = fixture.client.in_folder_router_auto_for_test(&session);
    fixture.client.shutdown().await;

    assert_eq!(failed.kind, ProviderSettingKind::Mode);
    assert_eq!(failed.value, "auto");
    assert_eq!(current_mode.as_deref(), Some("default"));
    assert!(
        matches!(held, Err(ExternalProviderRuntimeError::SettingsUnresolved)),
        "{held:?}"
    );
    assert_eq!(
        router_auto,
        Some(false),
        "a refused auto turns no allowance on"
    );
    assert_eq!(fixture.receipt()["prompts"], json!([]));
}

/// Oracle: D1 spec R2 row "Its answer is lost or unreadable" and R4 — the
/// allowance stays off while the mode is unconfirmed, and a confirmed set of
/// auto turns it on.
#[tokio::test]
async fn lost_auto_leaves_router_auto_off_until_confirmed() {
    let fixture = Fixture::start(&["malformed-mode-reply"]).await;

    let (session, failed) = created_without_settings_session(fixture.create("auto").await);
    let before = fixture.client.in_folder_router_auto_for_test(&session);
    fixture
        .client
        .set_setting(session.clone(), auto_mode())
        .await
        .expect("confirmed auto");
    let after = fixture.client.in_folder_router_auto_for_test(&session);
    fixture.client.shutdown().await;

    assert_eq!(
        failed.reason,
        session_event_model::ProviderSettingFailureReason::OutcomeUnknown
    );
    assert_eq!(before, Some(false), "a lost answer turns no allowance on");
    assert_eq!(after, Some(true), "the confirmed auto turns it on");
}

/// Oracle: D1 spec R4 "A start that ends with the session closed leaves no
/// allowance behind for it"; a control agent without session/close keeps
/// its session and its grant.
#[tokio::test]
async fn closed_invalid_start_leaves_no_in_folder_grant() {
    let closing = Fixture::start(&["advertise-close"]).await;
    let closed = closing.create("yolo").await;
    let closed_session = closing.receipt()["sessions"][0]["sessionId"]
        .as_str()
        .expect("session id")
        .to_owned();
    let closed_grant = closing
        .client
        .in_folder_router_auto_for_test(&closed_session);
    closing.client.shutdown().await;

    let control = Fixture::start(&[]).await;
    let kept = control.create("yolo").await;
    let kept_grant = control
        .client
        .in_folder_router_auto_for_test("fixture-session-1");
    control.client.shutdown().await;

    assert!(
        matches!(
            closed,
            Err(ExternalProviderRuntimeError::InvalidSetting {
                disposition: InvalidSettingSessionDisposition::Closed,
                ..
            })
        ),
        "{closed:?}"
    );
    assert_eq!(closing.receipt()["closes"], json!([closed_session]));
    assert_eq!(closed_grant, None, "the closed start leaves no grant");
    assert!(
        matches!(
            kept,
            Err(ExternalProviderRuntimeError::InvalidSetting {
                disposition: InvalidSettingSessionDisposition::RemainsCreated,
                ..
            })
        ),
        "{kept:?}"
    );
    assert!(
        kept_grant.is_some(),
        "a session that stays created keeps its grant"
    );
}
