//! The real Claude and Cursor behaviours drive the ACP runtime against a
//! scripted agent: which mode a new session gets (R3, R5), which folder it is
//! granted (R4), and which permission requests the Router answers itself
//! instead of asking (R4, R6).

#![expect(
    clippy::expect_used,
    reason = "A fixture that fails to start or answer must fail the test at once."
)]

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    ExternalProviderRuntimeError, HistoryReplayFuture, InteractionFuture, InteractionPort,
    PermissionModeReason, PermissionModeReport, ProviderBehavior, ProviderPersistenceTarget,
    RefusedApprovalOffer, RequestedProviderSettings, SessionEventSink,
};
use acp_provider_adapters::{ClaudeAcpBehavior, ClaudeSkillSource, CursorAcpBehavior};
use provider_session_machines::ProviderSettingKind;
use serde_json::{Value, json};
use session_event_model::{ApprovalRequest, SessionEvent};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

mod cursor_request_shapes;

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
        behaviours: &[&str],
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
        arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
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
                    ..Default::default()
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

/// An in-folder edit the fixture asks permission for.
fn in_folder_edit(folder: &Path) -> Value {
    json!({"kind":"edit","locations":[{"path":folder.join("note.txt")}]})
}

/// Oracle: D1 spec R2 row "Confirms auto" — `--mode auto` on Claude asks for
/// Claude's own `auto`; the agent confirms it, so the start reports actual
/// auto with no reason and the in-folder allowance is on.
#[tokio::test]
async fn claude_auto_runs_its_own_auto() {
    let fixture = Fixture::start(
        "claude",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        &[],
    )
    .await;
    let own = fixture.scratch("session-a");
    let auto = fixture
        .create(Some(&own), Some("auto"))
        .await
        .expect("created");
    fixture
        .ask("fixture-session-1", &in_folder_edit(&own))
        .await;
    let plan = fixture.create(None, Some("plan")).await.expect("created");
    let approvals_asked = fixture.approvals_asked();
    fixture.client.shutdown().await;
    assert_eq!(auto.permission_mode, report("auto", Some("auto"), None));
    assert_eq!(approvals_asked, 0, "the in-folder edit ran without asking");
    assert_eq!(plan.permission_mode, report("plan", Some("plan"), None));
    assert_eq!(
        fixture.receipt()["modeRequests"],
        json!([
            {"sessionId":"fixture-session-1","value":"auto"},
            {"sessionId":"fixture-session-2","value":"plan"},
        ])
    );
}

/// Oracle: D1 spec R2 row "Doesn't offer auto, offers acceptEdits" — actual
/// acceptEdits, reason autoNotOffered, allowance on.
#[tokio::test]
async fn claude_offering_only_accept_edits_reports_auto_not_offered() {
    let fixture = Fixture::start(
        "claude-without-auto",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        &[],
    )
    .await;
    let own = fixture.scratch("session-a");
    let created = fixture
        .create(Some(&own), Some("auto"))
        .await
        .expect("created");
    fixture
        .ask("fixture-session-1", &in_folder_edit(&own))
        .await;
    let approvals_asked = fixture.approvals_asked();
    fixture.client.shutdown().await;
    assert_eq!(
        created.permission_mode,
        report(
            "acceptEdits",
            Some("auto"),
            Some(PermissionModeReason::AutoNotOffered)
        )
    );
    assert_eq!(approvals_asked, 0, "the in-folder edit ran without asking");
    assert_eq!(
        fixture.receipt()["modeRequests"],
        json!([{"sessionId":"fixture-session-1","value":"acceptEdits"}])
    );
}

/// Oracle: D1 spec R2 row "Offers neither" and R4 — the starting mode stays,
/// reason autoNotOffered, and with no confirmed auto realization the
/// in-folder allowance is off, so an in-folder edit becomes an approval.
#[tokio::test]
async fn claude_offering_neither_keeps_its_mode_with_no_allowance() {
    let fixture = Fixture::start(
        "claude-without-auto-or-accept-edits",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        &[],
    )
    .await;
    let own = fixture.scratch("session-a");
    let created = fixture
        .create(Some(&own), Some("auto"))
        .await
        .expect("created");
    fixture
        .ask("fixture-session-1", &in_folder_edit(&own))
        .await;
    let approvals_asked = fixture.approvals_asked();
    fixture.client.shutdown().await;
    assert_eq!(
        created.permission_mode,
        report(
            "default",
            Some("auto"),
            Some(PermissionModeReason::AutoNotOffered)
        )
    );
    assert_eq!(approvals_asked, 1, "the in-folder edit became an approval");
    assert_eq!(fixture.receipt()["modeRequests"], json!([]));
}

/// Oracle: D1 spec R2 row "Confirms acceptEdits instead" — the actual mode is
/// Claude's own answer, so a requested auto that lands on its fallback is
/// reported as agentFellBack.
#[tokio::test]
async fn an_agent_that_lands_auto_on_its_fallback_is_reported_from_its_own_answer() {
    let fixture = Fixture::start(
        "claude",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        &["clamp-auto"],
    )
    .await;
    let own = fixture.scratch("session-a");
    let created = fixture
        .create(Some(&own), Some("auto"))
        .await
        .expect("created");
    // R2 "Confirms acceptEdits instead": the accepted fallback turns the
    // in-folder allowance on, so this edit runs without an approval.
    fixture
        .ask("fixture-session-1", &in_folder_edit(&own))
        .await;
    let approvals_asked = fixture.approvals_asked();
    fixture.client.shutdown().await;
    assert_eq!(
        created.permission_mode,
        report(
            "acceptEdits",
            Some("auto"),
            Some(PermissionModeReason::AgentFellBack)
        )
    );
    assert_eq!(approvals_asked, 0, "the in-folder edit ran without asking");
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
            &[],
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
        &[],
    )
    .await;
    let first = fixture.scratch("session-a");
    let second = fixture.scratch("session-ab");
    fixture
        .create(Some(&first), Some("auto"))
        .await
        .expect("first");
    fixture
        .create(Some(&second), Some("auto"))
        .await
        .expect("second");
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
        &[],
    )
    .await;
    let scratch = cursor.scratch("session-c");
    let created = cursor
        .create(Some(&scratch), Some("auto"))
        .await
        .expect("cursor");
    cursor.client.shutdown().await;
    assert_eq!(
        created.permission_mode,
        report(
            "agent",
            Some("auto"),
            Some(PermissionModeReason::AutoNotOffered)
        )
    );
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
        &[],
    )
    .await;
    let own = fixture.scratch("session-a");
    let other = fixture.scratch("session-ab");
    fixture
        .create(Some(&own), Some("auto"))
        .await
        .expect("first");
    fixture
        .create(Some(&other), Some("auto"))
        .await
        .expect("second");
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
        &[],
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

/// Oracle: R4/Q3 — the in-folder allowance follows the session's current mode:
/// a confirmed change from auto to a named mode makes in-folder actions ask, and
/// a change back to auto allows them again with the same folders.
#[tokio::test]
async fn the_in_folder_allowance_follows_a_confirmed_mode_change() {
    let fixture = Fixture::start(
        "cursor",
        "no-directories",
        Arc::new(CursorAcpBehavior),
        ApprovalPortOutcome::Selected {
            option_id: "reject".to_owned(),
            note: None,
        },
        &[],
    )
    .await;
    let own = fixture.scratch("session-a");
    fixture
        .create(Some(&own), Some("auto"))
        .await
        .expect("created in auto");
    let edit = json!({"kind":"edit","locations":[{"path":own.join("note.txt")}]});
    let change = |value: &str| acp_client_runtime::ProviderSettingRequest {
        kind: ProviderSettingKind::Mode,
        value: value.to_owned(),
        intent: session_event_model::SettingApplicationIntent::ExplicitChange,
        expected_model: None,
    };

    // Act: auto allows; after a change to `ask` the same edit asks.
    fixture.ask("fixture-session-1", &edit).await;
    assert_eq!(fixture.approvals_asked(), 0);
    fixture
        .client
        .set_setting("fixture-session-1".to_owned(), change("ask"))
        .await
        .expect("changed to ask");
    fixture.ask("fixture-session-1", &edit).await;
    assert_eq!(fixture.approvals_asked(), 1);

    // Act: back to Router auto, the edit is allowed again.
    fixture
        .client
        .set_setting("fixture-session-1".to_owned(), change("auto"))
        .await
        .expect("changed back to auto");
    fixture.ask("fixture-session-1", &edit).await;
    assert_eq!(fixture.approvals_asked(), 1);
    fixture.client.shutdown().await;
    let answers: Vec<Value> = fixture.receipt()["permissionAnswers"]
        .as_array()
        .expect("answers")
        .iter()
        .map(|answer| answer["answer"]["outcome"]["optionId"].clone())
        .collect();
    assert_eq!(
        answers,
        vec![json!("allow"), json!("reject"), json!("allow")]
    );
}

/// Oracle: as above, from a named mode at start to Router auto.
#[tokio::test]
async fn a_session_started_in_a_named_mode_gains_the_allowance_on_a_change_to_auto() {
    let fixture = Fixture::start(
        "cursor",
        "no-directories",
        Arc::new(CursorAcpBehavior),
        ApprovalPortOutcome::Cancelled,
        &[],
    )
    .await;
    let own = fixture.scratch("session-a");
    fixture
        .create(Some(&own), Some("ask"))
        .await
        .expect("created in ask");
    let edit = json!({"kind":"edit","locations":[{"path":own.join("note.txt")}]});
    fixture.ask("fixture-session-1", &edit).await;
    assert_eq!(fixture.approvals_asked(), 1);
    fixture
        .client
        .set_setting(
            "fixture-session-1".to_owned(),
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Mode,
                value: "auto".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
        .expect("changed to auto");
    fixture.ask("fixture-session-1", &edit).await;
    assert_eq!(fixture.approvals_asked(), 1);
    fixture.client.shutdown().await;
    assert_eq!(
        fixture.receipt()["permissionAnswers"][1]["answer"],
        json!({"outcome":{"outcome":"selected","optionId":"allow"}})
    );
}

/// Oracle: D1 spec R3 — Cursor's auto realization differs from auto, so a
/// requested auto carries the reason; a requested native `agent` carries
/// none. "No mode" is proven only by the Host refusal (V0).
#[tokio::test]
async fn cursor_auto_reports_its_stand_in_reason_and_its_own_agent_mode_none() {
    let fixture = Fixture::start(
        "cursor",
        "no-directories",
        Arc::new(CursorAcpBehavior),
        ApprovalPortOutcome::Cancelled,
        &[],
    )
    .await;
    let explicit = fixture
        .create(None, Some("auto"))
        .await
        .expect("explicit auto");
    let native = fixture
        .create(None, Some("agent"))
        .await
        .expect("native agent");
    fixture.client.shutdown().await;
    assert_eq!(
        explicit.permission_mode,
        report(
            "agent",
            Some("auto"),
            Some(PermissionModeReason::AutoNotOffered)
        )
    );
    assert_eq!(native.permission_mode, report("agent", Some("agent"), None));
}

/// A Claude start with the Host's fast default (off when offered).
async fn create_with_fast_default(
    fixture: &Fixture,
    scratch: Option<&Path>,
) -> Result<acp_client_runtime::ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
    fixture
        .client
        .create_session_with_settings(
            fixture.project.clone(),
            scratch.map(Path::to_path_buf),
            RequestedProviderSettings {
                mode: Some("auto".to_owned()),
                fast: acp_client_runtime::FastRequest::OffIfOffered,
                ..Default::default()
            },
        )
        .await
}

fn fast_change(value: &str) -> acp_client_runtime::ProviderSettingRequest {
    acp_client_runtime::ProviderSettingRequest {
        kind: ProviderSettingKind::Fast,
        value: value.to_owned(),
        intent: session_event_model::SettingApplicationIntent::ExplicitChange,
        expected_model: None,
    }
}

/// Oracle: D1 spec R10 — Claude offers no fast option, so the default does
/// nothing, sends no fast set, and reports fast: none.
#[tokio::test]
async fn claude_default_fast_is_a_no_op_and_reports_none() {
    let fixture = Fixture::start(
        "claude",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        &[],
    )
    .await;
    let created = create_with_fast_default(&fixture, None)
        .await
        .expect("created");
    fixture.client.shutdown().await;
    assert_eq!(
        created.effective_settings.fast,
        Some(acp_client_runtime::FastObserved::NoOption)
    );
    assert_eq!(fixture.receipt()["fastRequests"], json!([]));
}

/// Oracle: D1 spec R10 — an explicit fast off on Claude, which has no fast
/// option, is an invalid setting with no advertised choices, and nothing is
/// sent to the agent.
#[tokio::test]
async fn claude_explicit_fast_off_is_invalid() {
    let fixture = Fixture::start(
        "claude",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        &[],
    )
    .await;
    create_with_fast_default(&fixture, None)
        .await
        .expect("created");
    let requests_before = fixture.receipt()["modeRequests"].clone();
    let refused = fixture
        .client
        .set_setting("fixture-session-1".to_owned(), fast_change("off"))
        .await;
    fixture.client.shutdown().await;
    match refused {
        Err(ExternalProviderRuntimeError::InvalidSetting {
            setting: ProviderSettingKind::Fast,
            advertised,
            ..
        }) => assert!(advertised.is_empty(), "{advertised:?}"),
        other => panic!("expected an invalid fast setting, got {other:?}"),
    }
    assert_eq!(fixture.receipt()["fastRequests"], json!([]));
    assert_eq!(fixture.receipt()["modeRequests"], requests_before);
}

/// Oracle: D1 spec R10 "leaves no hold behind (even when another setting
/// failed at start)" — after a lost mode, nothing lists fast as not applied,
/// and once the mode is confirmed no hold is left: a new prompt runs.
#[tokio::test]
async fn claude_lost_mode_leaves_no_phantom_fast_hold() {
    let fixture = Fixture::start(
        "claude",
        "grants-directories",
        claude(),
        ApprovalPortOutcome::Cancelled,
        &["malformed-mode-reply"],
    )
    .await;
    let own = fixture.scratch("session-a");
    let not_applied = match create_with_fast_default(&fixture, Some(&own)).await {
        Err(ExternalProviderRuntimeError::CreatedWithoutSettings { not_applied, .. }) => {
            not_applied
        }
        other => panic!("expected a start created without settings, got {other:?}"),
    };
    fixture
        .client
        .set_setting(
            "fixture-session-1".to_owned(),
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Mode,
                value: "auto".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
        .expect("confirmed auto");
    fixture
        .ask("fixture-session-1", &in_folder_edit(&own))
        .await;
    fixture.client.shutdown().await;
    assert!(
        not_applied
            .iter()
            .all(|setting| setting.kind != ProviderSettingKind::Fast),
        "{not_applied:?}"
    );
    assert_eq!(fixture.receipt()["prompts"], json!(["fixture-session-1"]));
}
