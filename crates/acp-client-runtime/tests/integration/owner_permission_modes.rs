//! In-folder grants after start and restore, including how they answer ACP permission
//! requests when the interaction port refuses human approvals (D1 spec R2, R4).

#![expect(
    clippy::expect_used,
    reason = "A test fixture with missing wire evidence or settings must fail immediately."
)]

use crate::acp_owner_fixture::FixtureClient as AgentSessionClient;
use acp_client_runtime::{
    ApprovalPortOutcome, EventSinkClosed, ExternalProviderApprovalRefusalReason,
    ExternalProviderLaunch, ExternalProviderPromptOutcome, ExternalProviderRuntimeError,
    FastObserved, FastRequest, HistoryReplayFuture, InteractionFuture, InteractionPort,
    InvalidSettingSessionDisposition, ProviderPersistenceTarget, RefusedApprovalOffer,
    RequestedProviderSettings, SessionEventSink, SessionRestoreSetup, SessionWorkRoots,
};
use provider_session_machines::{
    ProviderSettingKind,
    session_lifecycle_machine::RestoreVia,
    session_owner::{ObservationCause, RestoreResult},
};
use serde_json::{Value, json};
use session_event_model::{
    ApprovalRequest, InputId, SessionEvent, StopReason,
    managed_interaction_refusal::ManagedInteractionRefusal,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const PERMISSION_MODE_AGENT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/permission-mode-agent.ts"
);

#[derive(Clone)]
struct PromptApprovalContext {
    input: InputId,
    binding_retirement: CancellationToken,
    managed_cancellation: CancellationToken,
}

struct CancellingApprovals(Option<ManagedInteractionRefusal>);
impl InteractionPort for CancellingApprovals {
    type Context = PromptApprovalContext;
    type OperationId = InputId;
    fn operation_id(context: &Self::Context) -> Self::OperationId {
        context.input.clone()
    }
    fn binding_retirement(context: &Self::Context) -> CancellationToken {
        context.binding_retirement.clone()
    }
    fn managed_refusal(&self) -> Option<ManagedInteractionRefusal> {
        self.0
    }
    fn cancel_after_managed_refusal(&self, context: Self::Context) -> InteractionFuture<'_, ()> {
        Box::pin(async move { context.managed_cancellation.cancel() })
    }
    fn request_approval(
        &self,
        _context: Self::Context,
        _request: ApprovalRequest,
        _turn: CancellationToken,
        _agent: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }
    fn request_question(
        &self,
        _context: Self::Context,
        _request: session_event_model::QuestionRequest,
        _turn: CancellationToken,
        _agent: CancellationToken,
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
        Self::start_in_profile("claude", behaviours).await
    }

    async fn start_in_profile(profile: &str, behaviours: &[&str]) -> Self {
        Self::start_with_approval_policy(profile, behaviours, None).await
    }

    async fn start_managed(profile: &str, behaviours: &[&str]) -> Self {
        Self::start_with_approval_policy(
            profile,
            behaviours,
            Some(ManagedInteractionRefusal::ApprovalsUnavailableOnBuild),
        )
        .await
    }

    async fn start_with_approval_policy(
        profile: &str,
        behaviours: &[&str],
        managed_refusal: Option<ManagedInteractionRefusal>,
    ) -> Self {
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
            profile.to_owned(),
            "grants-directories".to_owned(),
        ];
        arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
        let client = AgentSessionClient::initialize_with_mcp_http(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments,
                environment: Vec::new(),
                persistence_target: ProviderPersistenceTarget::Unspecified,
            },
            acp_client_runtime::ProviderModelPicker::Standard,
            "fixture",
            "http://127.0.0.1:1/mcp",
            Arc::new(CancellingApprovals(managed_refusal)),
            Arc::new(IgnoredEvents),
            Arc::new(crate::test_behavior::ClaudeLikeAutoBehavior),
            Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
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
    ) -> Result<acp_client_runtime::ExternalProviderCreatedSession, ExternalProviderRuntimeError>
    {
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
        self.prompt_edit(session, &self.scratch.join("note.txt"))
            .await
            .map(|_| ())
    }

    async fn prompt_edit(
        &self,
        session: &str,
        path: &Path,
    ) -> Result<(ExternalProviderPromptOutcome, CancellationToken), ExternalProviderRuntimeError>
    {
        let edit = json!({"kind":"edit","locations":[{"path":path}]});
        let input = InputId::generate();
        let managed_cancellation = CancellationToken::new();
        self.client
            .submit_with_approval_dispatch_for_input(
                session.to_owned(),
                input.clone(),
                acp_client_runtime::ProviderTurnInput::Prompt(
                    session_event_model::ProviderSubmission::plain(vec![
                        session_event_model::PromptContent::text(edit.to_string())
                            .expect("text prompt"),
                    ]),
                ),
                PromptApprovalContext {
                    input,
                    binding_retirement: CancellationToken::new(),
                    managed_cancellation: managed_cancellation.clone(),
                },
                None,
            )
            .await
            .map(|outcome| (outcome, managed_cancellation))
    }

    fn receipt(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.receipt).expect("receipt")).expect("JSON")
    }

    fn last_permission_reply(&self, session: &str) -> Value {
        let receipt = self.receipt();
        let answer = receipt["permissionAnswers"]
            .as_array()
            .and_then(|answers| answers.last())
            .expect("one permission reply for the prompt");
        assert_eq!(answer["sessionId"], session);
        answer["answer"].clone()
    }

    async fn assert_managed_allowance(&self, session: &str) {
        let (outcome, cancellation) = self
            .prompt_edit(session, &self.scratch.join("note.txt"))
            .await
            .expect("the prompt reaches its terminal response");
        assert_eq!(
            self.last_permission_reply(session),
            json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
            "an eligible grant selects the provider's allow-once option"
        );
        assert_eq!(outcome.stop_reason, StopReason::EndTurn);
        assert_eq!(outcome.permission_refusal_reason, None);
        assert!(
            !cancellation.is_cancelled(),
            "the allowed turn keeps running"
        );
    }

    async fn assert_managed_refusal(&self, session: &str, path: &Path) {
        let (outcome, cancellation) = self
            .prompt_edit(session, path)
            .await
            .expect("the prompt reaches its terminal response");
        assert_eq!(
            self.last_permission_reply(session),
            json!({"outcome":{"outcome":"cancelled"}}),
        );
        assert_eq!(
            outcome.permission_refusal_reason,
            Some(ExternalProviderApprovalRefusalReason::Managed(
                ManagedInteractionRefusal::ApprovalsUnavailableOnBuild
            )),
        );
        assert!(
            cancellation.is_cancelled(),
            "the managed port cancels the refused turn"
        );
    }
}

impl Fixture {
    fn saved_setup(&self, mode: &str) -> SessionRestoreSetup {
        SessionRestoreSetup {
            requested_mode: Some(mode.to_owned()),
            scratch_folder: Some(self.scratch.clone()),
            work_roots: SessionWorkRoots {
                router_auto: mode == "auto",
                ..Default::default()
            },
        }
    }

    async fn restore(
        &self,
        session: &str,
        setup: SessionRestoreSetup,
        via: RestoreVia,
    ) -> RestoreResult {
        self.client
            .restore(session.to_owned(), self.project.clone(), setup, via)
            .await
            .expect("the owner restores the session")
    }

    fn current_mode(&self, session: &str) -> String {
        self.client
            .settings_catalog(session)
            .expect("catalog of the owner run")
            .effective_settings()
            .mode
            .expect("fixture reports a mode")
    }

    fn grant(&self, session: &str) -> Option<bool> {
        self.client.in_folder_router_auto_for_test(session)
    }

    fn mode_requests(&self, session: &str) -> Vec<String> {
        self.receipt()
            .get("modeRequests")
            .and_then(Value::as_array)
            .expect("mode requests")
            .iter()
            .filter(|request| request.get("sessionId").and_then(Value::as_str) == Some(session))
            .map(|request| {
                request
                    .get("value")
                    .and_then(Value::as_str)
                    .expect("mode value")
                    .to_owned()
            })
            .collect()
    }
}

#[tokio::test]
async fn refused_auto_reports_the_mode_without_holding_input_or_granting_allowance() {
    let fixture = Fixture::start(&["reject-mode"]).await;
    let created = fixture.create("auto").await;
    let Err(ExternalProviderRuntimeError::CreatedWithoutSettings {
        provider_session_id: session,
        failed,
        ..
    }) = created
    else {
        panic!("a definite Auto refusal keeps the session: {created:?}");
    };

    assert_eq!(failed.kind, ProviderSettingKind::Mode);
    assert_eq!(failed.value, "auto");
    assert_eq!(fixture.current_mode(&session), "default");
    assert_eq!(fixture.grant(&session), Some(false));
    fixture
        .prompt_in_folder_edit(&session)
        .await
        .expect("the known refusal leaves input available");
    let receipt = fixture.receipt();
    assert_eq!(receipt["prompts"], json!([session]));
    assert_eq!(
        receipt.pointer("/permissionAnswers/0/answer/outcome/outcome"),
        Some(&json!("cancelled"))
    );
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn closed_invalid_start_leaves_no_in_folder_grant() {
    let closing = Fixture::start(&["advertise-close"]).await;
    let closed = closing.create("yolo").await;
    let closed_session = closing.receipt()["sessions"][0]["sessionId"]
        .as_str()
        .expect("session ID")
        .to_owned();
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
    assert_eq!(closing.grant(&closed_session), None);
    closing.client.shutdown().await;

    let control = Fixture::start(&[]).await;
    let kept = control.create("yolo").await;
    let kept_session = control.receipt()["sessions"][0]["sessionId"]
        .as_str()
        .expect("session ID")
        .to_owned();
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
    assert!(control.grant(&kept_session).is_some());
    control.client.shutdown().await;
}

#[tokio::test]
async fn a_restored_auto_session_gets_its_folders_and_an_active_grant() {
    let fixture = Fixture::start(&["advertise-restore", "restored-in-auto"]).await;
    let cases = [
        ("loaded-session", RestoreVia::Load, "session/load"),
        ("resumed-session", RestoreVia::Resume, "session/resume"),
    ];
    for (id, via, _) in cases {
        fixture.restore(id, fixture.saved_setup("auto"), via).await;
        assert_eq!(fixture.current_mode(id), "auto");
        assert!(fixture.mode_requests(id).is_empty());
        assert_eq!(fixture.grant(id), Some(true));
    }
    let restores = fixture.receipt()["restores"].clone();
    let restores = restores.as_array().expect("restore requests");
    assert_eq!(restores.len(), cases.len());
    for (restore, (id, _, method)) in restores.iter().zip(cases) {
        assert_eq!(restore["sessionId"], id);
        assert_eq!(restore["method"], method);
        assert_eq!(restore["additionalDirectories"], json!([fixture.scratch]));
    }
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn auto_only_allows_in_folder_edits_under_managed_refusal() {
    let fixture = Fixture::start_managed("claude", &[]).await;
    let session = fixture
        .create("auto")
        .await
        .expect("created in Auto")
        .provider_session_id;
    fixture.assert_managed_allowance(&session).await;
    let outside = fixture
        .scratch
        .parent()
        .expect("scratch parent")
        .join("other.txt");
    fixture.assert_managed_refusal(&session, &outside).await;
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn default_refuses_in_folder_edits_under_managed_refusal() {
    let fixture = Fixture::start_managed("claude", &[]).await;
    let session = fixture
        .create("default")
        .await
        .expect("created in default")
        .provider_session_id;
    fixture
        .assert_managed_refusal(&session, &fixture.scratch.join("note.txt"))
        .await;
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn a_session_the_agent_restores_out_of_auto_is_set_back_to_auto() {
    let fixture = Fixture::start_managed("claude", &["advertise-restore"]).await;
    let id = "resumed-session";
    fixture
        .restore(id, fixture.saved_setup("auto"), RestoreVia::Resume)
        .await;
    assert_eq!(fixture.mode_requests(id), ["auto"]);
    assert_eq!(
        fixture.grant(id),
        Some(true),
        "the ready run grants its confirmed Auto mode"
    );
    assert_eq!(fixture.current_mode(id), "auto");
    fixture.assert_managed_allowance(id).await;
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn a_session_recorded_in_default_that_restores_in_auto_ends_in_default() {
    let fixture =
        Fixture::start_managed("claude", &["advertise-restore", "restored-in-auto"]).await;
    let id = "loaded-session";
    fixture
        .restore(id, fixture.saved_setup("default"), RestoreVia::Load)
        .await;
    assert_eq!(fixture.mode_requests(id), ["default"]);
    assert_eq!(
        fixture.grant(id),
        Some(false),
        "the ready run cannot use the old permissive scope"
    );
    assert_eq!(fixture.current_mode(id), "default");
    fixture
        .assert_managed_refusal(id, &fixture.scratch.join("note.txt"))
        .await;
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn a_restore_whose_mode_set_fails_loads_without_a_grant_and_shows_the_mismatch() {
    let fixture = Fixture::start_managed("claude", &["advertise-restore", "reject-mode"]).await;
    let id = "resumed-session";
    fixture
        .restore(id, fixture.saved_setup("auto"), RestoreVia::Resume)
        .await;
    assert_eq!(fixture.mode_requests(id), ["auto"]);
    assert_eq!(fixture.current_mode(id), "default");
    assert_eq!(fixture.grant(id), Some(false));
    fixture
        .assert_managed_refusal(id, &fixture.scratch.join("note.txt"))
        .await;
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn a_refused_stricter_restore_reports_auto_without_granting_it() {
    let fixture = Fixture::start_managed(
        "claude",
        &["advertise-restore", "restored-in-auto", "reject-mode"],
    )
    .await;
    let id = "loaded-session";
    let restored = fixture
        .restore(id, fixture.saved_setup("default"), RestoreVia::Load)
        .await;
    assert_eq!(fixture.mode_requests(id), ["default"]);
    assert_eq!(fixture.current_mode(id), "auto");
    assert_eq!(fixture.grant(id), Some(false));
    assert_eq!(
        restored
            .observed
            .get(&ProviderSettingKind::Mode)
            .and_then(|evidence| evidence.cause),
        Some(ObservationCause::Refused),
    );
    fixture
        .assert_managed_refusal(id, &fixture.scratch.join("note.txt"))
        .await;
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn a_refused_default_restore_never_grants_the_agents_auto_fallback() {
    let fixture = Fixture::start_managed(
        "claude-without-auto",
        &[
            "advertise-restore",
            "restored-in-accept-edits",
            "reject-mode",
        ],
    )
    .await;
    let id = "loaded-session";
    let restored = fixture
        .restore(id, fixture.saved_setup("default"), RestoreVia::Load)
        .await;
    assert_eq!(fixture.mode_requests(id), ["default"]);
    assert_eq!(fixture.current_mode(id), "acceptEdits");
    assert_eq!(
        fixture.grant(id),
        Some(false),
        "the creator did not ask for Router Auto"
    );
    assert_eq!(
        restored
            .observed
            .get(&ProviderSettingKind::Mode)
            .and_then(|evidence| evidence.cause),
        Some(ObservationCause::Refused),
    );
    fixture
        .assert_managed_refusal(id, &fixture.scratch.join("note.txt"))
        .await;
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn a_restore_without_a_recorded_setup_grants_nothing() {
    let fixture = Fixture::start(&["advertise-restore", "restored-in-auto"]).await;
    let id = "old-session";
    fixture
        .restore(id, SessionRestoreSetup::default(), RestoreVia::Resume)
        .await;
    let receipt = fixture.receipt();
    let directories = &receipt["restores"][0]["additionalDirectories"];
    assert!(
        directories.is_null() || *directories == json!([]),
        "{receipt}"
    );
    assert!(fixture.mode_requests(id).is_empty());
    assert_eq!(fixture.grant(id), None);
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn a_restore_without_a_grant_leaves_no_earlier_runs_allowance() {
    let fixture = Fixture::start(&["advertise-restore"]).await;
    let created = fixture.create("auto").await.expect("created in Auto");
    let id = created.provider_session_id;
    assert_eq!(fixture.grant(&id), Some(true));
    fixture
        .client
        .end_session_actor(id.clone())
        .await
        .expect("retired Auto run");

    fixture
        .restore(
            &id,
            SessionRestoreSetup {
                requested_mode: Some("default".to_owned()),
                scratch_folder: Some(fixture.project.join("removed-scratch")),
                work_roots: SessionWorkRoots::default(),
            },
            RestoreVia::Resume,
        )
        .await;

    assert_eq!(fixture.current_mode(&id), "default");
    assert_eq!(fixture.grant(&id), None);
    fixture
        .prompt_in_folder_edit(&id)
        .await
        .expect("the restored input runs");
    assert_eq!(
        fixture
            .receipt()
            .pointer("/permissionAnswers/0/answer/outcome/outcome"),
        Some(&json!("cancelled")),
    );
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn a_restored_session_keeps_fast_off() {
    let fixture = Fixture::start(&["offer-fast", "advertise-restore", "restore-fast-on"]).await;
    let created = fixture
        .client
        .create_session_with_settings(
            fixture.project.clone(),
            Some(fixture.scratch.clone()),
            RequestedProviderSettings {
                mode: Some("auto".to_owned()),
                fast: FastRequest::OffIfOffered,
                ..Default::default()
            },
        )
        .await
        .expect("created with Fast off");
    let id = created.provider_session_id;
    fixture
        .client
        .end_session_actor(id.clone())
        .await
        .expect("retired original run");
    fixture
        .restore(&id, fixture.saved_setup("auto"), RestoreVia::Resume)
        .await;

    assert_eq!(
        fixture.receipt()["fastRequests"],
        json!([
            {"sessionId": id, "value": "false"},
            {"sessionId": id, "value": "false"},
        ])
    );
    assert_eq!(
        fixture
            .client
            .settings_catalog(&id)
            .expect("owner catalog")
            .fast_observed(),
        FastObserved::Off,
    );
    fixture.client.shutdown().await;
}

#[tokio::test]
async fn an_unoffered_saved_mode_still_accepts_input_on_the_restored_session() {
    let fixture = Fixture::start_in_profile(
        "claude-without-auto-or-accept-edits",
        &["advertise-restore", "restored-in-auto"],
    )
    .await;
    let id = "resumed-session";
    fixture
        .restore(id, fixture.saved_setup("acceptEdits"), RestoreVia::Resume)
        .await;
    assert_eq!(fixture.current_mode(id), "auto");
    assert!(fixture.mode_requests(id).is_empty());
    fixture
        .prompt_in_folder_edit(id)
        .await
        .expect("the restored session accepts input");
    fixture
        .client
        .accept_session_settings(id.to_owned())
        .await
        .expect("Accept after restore");
    let receipt = fixture.receipt();
    assert_eq!(receipt["prompts"], json!([id]));
    assert_eq!(
        receipt.pointer("/permissionAnswers/0/answer/outcome/outcome"),
        Some(&json!("cancelled")),
    );
    fixture.client.shutdown().await;
}
