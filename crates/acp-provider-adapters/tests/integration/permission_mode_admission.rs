//! The real Claude and Cursor behaviours drive the ACP runtime against a
//! scripted agent: which mode a new session gets (R3, R5), which folder it is
//! granted (R4), and which permission requests the Router answers itself
//! instead of asking (R4, R6).

#![expect(
    clippy::expect_used,
    reason = "A fixture that fails to start or answer must fail the test at once."
)]

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, AutoPermissionMode, AutoRealization, EventSinkClosed,
    ExternalProviderLaunch, ExternalProviderRuntimeError, HistoryReplayFuture, InteractionFuture,
    InteractionPort, PermissionModeReason, PermissionModeReport, ProviderBehavior,
    ProviderPersistenceTarget, ProviderSettingKind, RefusedApprovalOffer,
    RequestedProviderSettings, SessionEventSink,
};
use acp_provider_adapters::{ClaudeAcpBehavior, ClaudeSkillSource, CursorAcpBehavior};
use serde_json::{Value, json};
use session_event_model::{ApprovalRequest, SessionEvent};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

const PERMISSION_MODE_AGENT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/permission-mode-agent.ts"
);

/// Records every approval that reaches the Router's approval path and
/// answers it with the outcome the test chose.
struct RecordingApprovals {
    asked: Mutex<Vec<ApprovalRequest>>,
    answer: ApprovalPortOutcome,
}

impl InteractionPort for RecordingApprovals {
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
        request: ApprovalRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(request);
        }
        let answer = self.answer.clone();
        Box::pin(async move { answer })
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

/// An agent whose own `auto` may land on its ask-first mode.
struct FallingBackAuto(CursorAcpBehavior);
impl ProviderBehavior for FallingBackAuto {
    fn initialization_authentication(
        &self,
    ) -> acp_client_runtime::ProviderInitializationAuthentication {
        acp_client_runtime::ProviderInitializationAuthentication::NotRequested
    }
    fn can_reuse_setting_confirmation(&self, kind: ProviderSettingKind) -> bool {
        self.0.can_reuse_setting_confirmation(kind)
    }
    fn model_metadata_source(&self) -> acp_client_runtime::ModelMetadataSource {
        acp_client_runtime::ModelMetadataSource::Unavailable
    }
    fn decode_model_metadata(&self, value: &Value) -> acp_client_runtime::ModelReasoningMetadata {
        self.0.decode_model_metadata(value)
    }
    fn auto_permission_mode(&self) -> AutoPermissionMode {
        AutoPermissionMode {
            realized_by: Some("auto"),
            fallback: Some("acceptEdits"),
            realization: AutoRealization::ClassifierEscalates,
            stand_in_reason: None,
        }
    }
    fn observe(
        &self,
        update: acp_client_runtime::ProviderUpdate,
    ) -> Result<acp_client_runtime::ProviderFacts, acp_client_runtime::ProviderError> {
        self.0.observe(update)
    }
    fn prepare(
        &self,
        submission: session_event_model::ProviderSubmission,
    ) -> Result<Vec<session_event_model::PromptContent>, acp_client_runtime::ProviderError> {
        self.0.prepare(submission)
    }
    fn compact(
        &self,
    ) -> Result<acp_client_runtime::PreparedCompact, acp_client_runtime::ProviderError> {
        self.0.compact()
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    scratch_root: PathBuf,
    receipt: PathBuf,
    approvals: Arc<RecordingApprovals>,
    client: AgentSessionClient<RecordingApprovals>,
}

impl Fixture {
    async fn start(
        profile: &str,
        directories: &str,
        behavior: Arc<dyn ProviderBehavior>,
        answer: ApprovalPortOutcome,
        clamp_auto: bool,
    ) -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let base = std::fs::canonicalize(root.path()).expect("canonical root");
        let project = base.join("project");
        let scratch_root = base.join("scratch");
        std::fs::create_dir_all(&project).expect("project");
        std::fs::create_dir_all(&scratch_root).expect("scratch root");
        let receipt = base.join("receipt.json");
        let mut arguments = vec![
            PERMISSION_MODE_AGENT.to_owned(),
            receipt.display().to_string(),
            profile.to_owned(),
            directories.to_owned(),
        ];
        if clamp_auto {
            arguments.push("clamp-auto".to_owned());
        }
        let approvals = Arc::new(RecordingApprovals {
            asked: Mutex::new(Vec::new()),
            answer,
        });
        let client = AgentSessionClient::initialize(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments,
                environment: Vec::new(),
                persistence_target: ProviderPersistenceTarget::CursorAllowlist,
            },
            Arc::clone(&approvals),
            Arc::new(NoopEventSink),
            behavior,
        )
        .await
        .expect("fixture initializes");
        Self {
            _root: root,
            project,
            scratch_root,
            receipt,
            approvals,
            client,
        }
    }

    fn scratch(&self, name: &str) -> PathBuf {
        let folder = self.scratch_root.join(name);
        std::fs::create_dir_all(&folder).expect("scratch folder");
        folder
    }

    async fn create(
        &self,
        scratch: Option<&Path>,
        mode: Option<&str>,
    ) -> Result<acp_client_runtime::ExternalProviderCreatedSession, ExternalProviderRuntimeError>
    {
        self.client
            .create_session_with_settings(
                self.project.clone(),
                scratch.map(Path::to_path_buf),
                RequestedProviderSettings {
                    mode: mode.map(str::to_owned),
                    model: None,
                    effort: None,
                },
            )
            .await
    }

    /// Prompts the session; the fixture asks permission for `tool_call`.
    async fn ask(&self, session: &str, tool_call: &Value) {
        self.client
            .submit_with_approval_dispatch_for_input(
                session.to_owned(),
                session_event_model::InputId::generate(),
                acp_client_runtime::ProviderTurnInput::Prompt(
                    session_event_model::ProviderSubmission::plain(vec![
                        session_event_model::PromptContent::text(tool_call.to_string())
                            .expect("text prompt"),
                    ]),
                ),
                (),
                None,
            )
            .await
            .expect("prompt completes");
    }

    fn receipt(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.receipt).expect("receipt")).expect("JSON")
    }

    fn approvals_asked(&self) -> usize {
        self.approvals.asked.lock().expect("approval lock").len()
    }
}

fn claude() -> Arc<dyn ProviderBehavior> {
    Arc::new(ClaudeAcpBehavior::new(ClaudeSkillSource::Annotated))
}

fn report(
    actual: &str,
    requested: Option<&str>,
    reason: Option<PermissionModeReason>,
) -> PermissionModeReport {
    PermissionModeReport {
        requested: requested.map(str::to_owned),
        actual: Some(actual.to_owned()),
        reason,
    }
}

/// Oracle: spec R3 (no mode means auto, with the actual mode and why) and the
/// program-design Claude row as decided 2026-11-14 (auto stands in as
/// acceptEdits until escalation is proven).
#[tokio::test]
async fn claude_without_a_mode_runs_its_stand_in_and_says_why() {
    let fixture = Fixture::start(
        "claude",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        false,
    )
    .await;
    let unnamed = fixture.create(None, None).await.expect("created");
    assert_eq!(
        unnamed.permission_mode,
        report(
            "acceptEdits",
            None,
            Some(PermissionModeReason::EscalationUnproven)
        )
    );
    let auto = fixture.create(None, Some("auto")).await.expect("created");
    assert_eq!(
        auto.permission_mode,
        report(
            "acceptEdits",
            Some("auto"),
            Some(PermissionModeReason::EscalationUnproven)
        )
    );
    let plan = fixture.create(None, Some("plan")).await.expect("created");
    assert_eq!(plan.permission_mode, report("plan", Some("plan"), None));
    fixture.client.shutdown().await;
    assert_eq!(
        fixture.receipt()["modeRequests"],
        json!([
            {"sessionId":"fixture-session-1","value":"acceptEdits"},
            {"sessionId":"fixture-session-2","value":"acceptEdits"},
            {"sessionId":"fixture-session-3","value":"plan"},
        ])
    );
}

/// Oracle: spec R3 — the actual mode is the agent's own answer, so an agent
/// that lands its auto on the fallback is reported as having fallen back.
#[tokio::test]
async fn an_agent_that_lands_auto_on_its_fallback_is_reported_from_its_own_answer() {
    let fixture = Fixture::start(
        "claude",
        "grants-directories",
        Arc::new(FallingBackAuto(CursorAcpBehavior)),
        ApprovalPortOutcome::Cancelled,
        true,
    )
    .await;
    let created = fixture.create(None, None).await.expect("created");
    fixture.client.shutdown().await;
    assert_eq!(
        created.permission_mode,
        report(
            "acceptEdits",
            None,
            Some(PermissionModeReason::AgentFellBack)
        )
    );
    assert_eq!(
        fixture.receipt()["modeRequests"],
        json!([{"sessionId":"fixture-session-1","value":"auto"}])
    );
}

/// Oracle: spec R5 — `auto` or the agent's own names; anything else is
/// refused with the accepted list and no mode is sent to the agent.
#[tokio::test]
async fn bogus_and_foreign_mode_names_are_refused_with_the_accepted_list() {
    for (profile, behavior, value, accepted) in [
        (
            "claude",
            claude(),
            "yolo",
            vec!["auto", "default", "acceptEdits", "plan"],
        ),
        (
            "cursor",
            Arc::new(CursorAcpBehavior) as Arc<dyn ProviderBehavior>,
            "acceptEdits",
            vec!["auto", "agent", "plan", "ask"],
        ),
    ] {
        let fixture = Fixture::start(
            profile,
            "grants-directories",
            behavior,
            ApprovalPortOutcome::Cancelled,
            false,
        )
        .await;
        let refused = fixture.create(None, Some(value)).await;
        fixture.client.shutdown().await;
        match refused {
            Err(ExternalProviderRuntimeError::InvalidSetting {
                setting: ProviderSettingKind::Mode,
                value: refused_value,
                advertised,
                ..
            }) => {
                assert_eq!(refused_value, value, "{profile}");
                assert_eq!(advertised, accepted, "{profile}");
            }
            other => panic!("{profile}: expected a mode refusal, got {other:?}"),
        }
        assert_eq!(fixture.receipt()["modeRequests"], json!([]), "{profile}");
    }
}

/// Oracle: spec R4 as corrected in r2 — each session is granted exactly its
/// own scratch folder, never the system temp folders, and only when the agent
/// accepts additional directories.
#[tokio::test]
async fn each_session_is_granted_only_its_own_scratch_folder() {
    let fixture = Fixture::start(
        "claude",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        false,
    )
    .await;
    let first = fixture.scratch("session-a");
    let second = fixture.scratch("session-ab");
    fixture.create(Some(&first), None).await.expect("first");
    fixture.create(Some(&second), None).await.expect("second");
    fixture.client.shutdown().await;
    let sessions = fixture.receipt()["sessions"].clone();
    assert_eq!(
        sessions[0]["additionalDirectories"],
        json!([first.display().to_string()])
    );
    assert_eq!(
        sessions[1]["additionalDirectories"],
        json!([second.display().to_string()])
    );

    let cursor = Fixture::start(
        "cursor",
        "no-directories",
        Arc::new(CursorAcpBehavior),
        ApprovalPortOutcome::Cancelled,
        false,
    )
    .await;
    let scratch = cursor.scratch("session-c");
    let created = cursor.create(Some(&scratch), None).await.expect("cursor");
    cursor.client.shutdown().await;
    assert_eq!(created.permission_mode, report("agent", None, None));
    assert_eq!(
        cursor.receipt()["sessions"][0]["additionalDirectories"],
        Value::Null
    );
}

/// Oracle: R4/Q3 and R6 — inside the session's own folders the Router allows
/// once without asking; anything else, including another session's folder,
/// reaches the approval path and the agent gets that approval's answer.
#[tokio::test]
async fn in_folder_actions_are_allowed_and_everything_else_becomes_an_approval() {
    let fixture = Fixture::start(
        "cursor",
        "no-directories",
        Arc::new(CursorAcpBehavior),
        ApprovalPortOutcome::Selected {
            option_id: "reject".to_owned(),
            note: None,
        },
        false,
    )
    .await;
    let own = fixture.scratch("session-a");
    let other = fixture.scratch("session-ab");
    fixture.create(Some(&own), None).await.expect("first");
    fixture.create(Some(&other), None).await.expect("second");
    let edit = |path: &Path| json!({"kind":"edit","locations":[{"path":path}]});
    let shell = |command: String| json!({"kind":"execute","rawInput":{"command":command}});
    let cases = [
        (edit(&own.join("note.txt")), false),
        (edit(&fixture.project.join("src/lib.rs")), false),
        (shell(format!("rm {}/note.txt", own.display())), false),
        (edit(&other.join("note.txt")), true),
        (edit(Path::new("/tmp/d1-outside")), true),
        (shell("touch /tmp/d1-outside".to_owned()), true),
    ];
    let mut expected_asked = 0;
    let mut expected_answers = Vec::new();
    for (tool_call, asks) in &cases {
        fixture.ask("fixture-session-1", tool_call).await;
        if *asks {
            expected_asked += 1;
        }
        assert_eq!(fixture.approvals_asked(), expected_asked, "{tool_call}");
        let option = if *asks { "reject" } else { "allow" };
        expected_answers.push(json!({
            "sessionId":"fixture-session-1",
            "answer":{"outcome":{"outcome":"selected","optionId":option}},
        }));
    }
    fixture.client.shutdown().await;
    assert_eq!(
        fixture.receipt()["permissionAnswers"],
        json!(expected_answers)
    );
}

/// Oracle: R4/Q3 — the allowance belongs to auto mode; a session started in
/// one of the agent's own modes asks for everything.
#[tokio::test]
async fn a_session_in_a_named_mode_has_no_in_folder_allowance() {
    let fixture = Fixture::start(
        "cursor",
        "no-directories",
        Arc::new(CursorAcpBehavior),
        ApprovalPortOutcome::Cancelled,
        false,
    )
    .await;
    let own = fixture.scratch("session-a");
    fixture
        .create(Some(&own), Some("ask"))
        .await
        .expect("created");
    fixture
        .ask(
            "fixture-session-1",
            &json!({"kind":"edit","locations":[{"path":own.join("note.txt")}]}),
        )
        .await;
    assert_eq!(fixture.approvals_asked(), 1);
    fixture.client.shutdown().await;
    assert_eq!(
        fixture.receipt()["permissionAnswers"][0]["answer"],
        json!({"outcome":{"outcome":"cancelled"}})
    );
}
