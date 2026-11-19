//! Which in-folder grant a start or a restore leaves behind, read from the grant map the
//! permission path uses (D1 spec R2, R4).

use crate::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    ExternalProviderRuntimeError, HistoryReplayFuture, InteractionFuture, InteractionPort,
    InvalidSettingSessionDisposition, ProviderPersistenceTarget, ProviderSettingRequest,
    RefusedApprovalOffer, RequestedProviderSettings, SessionEventSink,
};
use provider_session_machines::ProviderSettingKind;
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
        Self::start_in_profile("claude", behaviours).await
    }

    async fn start_in_profile(profile: &str, behaviours: &[&str]) -> Self {
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

/// A refused requested mode is reported and keeps the agent's known
/// starting mode. Input may run without acquiring the auto allowance.
#[tokio::test]
async fn refused_auto_reports_the_mode_without_holding_input_or_granting_allowance() {
    let fixture = Fixture::start(&["reject-mode"]).await;

    let (session, failed) = created_without_settings_session(fixture.create("auto").await);
    let prompted = fixture.prompt_in_folder_edit(&session).await;
    let current_mode = fixture
        .client
        .settings_catalog(&session)
        .expect("catalog")
        .effective_settings()
        .mode;
    let router_auto = fixture.client.in_folder_router_auto_for_test(&session);
    fixture.client.shutdown().await;

    assert_eq!(failed.kind, ProviderSettingKind::Mode);
    assert_eq!(failed.value, "auto");
    assert_eq!(current_mode.as_deref(), Some("default"));
    prompted.expect("the known refusal leaves input available");
    assert_eq!(
        router_auto,
        Some(false),
        "a refused auto turns no allowance on"
    );
    let receipt = fixture.receipt();
    assert_eq!(receipt["prompts"], json!([session]));
    assert_eq!(
        receipt
            .pointer("/permissionAnswers/0/answer/outcome/outcome")
            .and_then(Value::as_str),
        Some("cancelled")
    );
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

impl Fixture {
    /// The setup a create in `mode` with this fixture's scratch folder had.
    fn restore_setup(&self, mode: Option<&str>) -> crate::SessionRestoreSetup {
        mode.map_or_else(crate::SessionRestoreSetup::default, |mode| {
            crate::SessionRestoreSetup {
                requested_mode: Some(mode.to_owned()),
                scratch_folder: Some(self.scratch.clone()),
                work_roots: crate::SessionWorkRoots {
                    router_auto: mode == "auto",
                    ..crate::SessionWorkRoots::default()
                },
            }
        })
    }
}

/// Oracle: owner Decision 3 and D1 R18, a resumed session gets back the
/// folders and the allowance its create had: a load and a resume of a session
/// created in auto send its scratch folder as an additional directory, and
/// leave an active in-folder grant when the agent restored it in auto; each
/// reports the requested mode auto. The agent already runs the recorded mode,
/// so no mode change is sent (Lead V6: skipped when actual equals recorded).
#[tokio::test]
async fn a_restored_auto_session_gets_its_folders_and_an_active_grant() {
    // Arrange
    let fixture = Fixture::start(&["advertise-restore", "restored-in-auto"]).await;

    // Act
    let loaded = fixture
        .client
        .load_session_with_setup(
            "loaded-session".to_owned(),
            fixture.project.clone(),
            fixture.restore_setup(Some("auto")),
        )
        .await
        .expect("load");
    let resumed = fixture
        .client
        .resume_session_with_setup(
            "resumed-session".to_owned(),
            fixture.project.clone(),
            fixture.restore_setup(Some("auto")),
        )
        .await
        .expect("resume");
    let loaded_grant = fixture
        .client
        .in_folder_router_auto_for_test("loaded-session");
    let resumed_grant = fixture
        .client
        .in_folder_router_auto_for_test("resumed-session");
    fixture.client.shutdown().await;

    // Assert
    let receipt = fixture.receipt();
    for session in ["loaded-session", "resumed-session"] {
        assert!(
            mode_requests_for(&receipt, session).is_empty(),
            "a restore already in its recorded mode sends no mode change"
        );
    }
    let restores = receipt["restores"].clone();
    assert_eq!(restores[0]["method"], "session/load");
    assert_eq!(restores[1]["method"], "session/resume");
    for restore in [&restores[0], &restores[1]] {
        assert_eq!(
            restore["additionalDirectories"],
            json!([fixture.scratch.display().to_string()]),
            "{restore}"
        );
    }
    assert_eq!(loaded_grant, Some(true));
    assert_eq!(resumed_grant, Some(true));
    for restored in [&loaded, &resumed] {
        assert_eq!(restored.permission_mode.requested.as_deref(), Some("auto"));
        assert_eq!(restored.permission_mode.actual.as_deref(), Some("auto"));
    }
}

/// The mode values the agent was asked to switch `session` to, in order.
fn mode_requests_for(receipt: &Value, session: &str) -> Vec<String> {
    receipt["modeRequests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|request| request["sessionId"] == session)
        .filter_map(|request| request["value"].as_str().map(str::to_owned))
        .collect()
}

/// Oracle: owner Decision 3 (a session's mode survives a restart) and the
/// Lead's V6 ruling, replacing D1 R4's "grant off until its mode is auto
/// again" for restores: a session recorded in auto that the agent restores in
/// another mode is set back to auto before it registers, and its grant is
/// judged on the mode then in force, active.
#[tokio::test]
async fn a_session_the_agent_restores_out_of_auto_is_set_back_to_auto() {
    // Arrange: the agent restores every session in its start mode, default.
    let fixture = Fixture::start(&["advertise-restore"]).await;

    // Act
    let resumed = fixture
        .client
        .resume_session_with_setup(
            "resumed-session".to_owned(),
            fixture.project.clone(),
            fixture.restore_setup(Some("auto")),
        )
        .await
        .expect("resume");
    let grant = fixture
        .client
        .in_folder_router_auto_for_test("resumed-session");
    fixture.client.shutdown().await;

    // Assert
    assert_eq!(
        mode_requests_for(&fixture.receipt(), "resumed-session"),
        vec!["auto".to_owned()],
        "the restore sets the recorded mode back"
    );
    assert_eq!(grant, Some(true));
    assert_eq!(resumed.permission_mode.requested.as_deref(), Some("auto"));
    assert_eq!(resumed.permission_mode.actual.as_deref(), Some("auto"));
}

/// Oracle: the Lead's V6 finding and ruling (owner Decision 3): a session
/// created in a named mode, default, that the agent restores in auto (as
/// Claude applies the user's defaultMode on load) ends in default, set back
/// before it registers, and never holds the Router's in-folder allowance.
#[tokio::test]
async fn a_session_recorded_in_default_that_restores_in_auto_ends_in_default() {
    // Arrange
    let fixture = Fixture::start(&["advertise-restore", "restored-in-auto"]).await;

    // Act
    let loaded = fixture
        .client
        .load_session_with_setup(
            "loaded-session".to_owned(),
            fixture.project.clone(),
            fixture.restore_setup(Some("default")),
        )
        .await
        .expect("load");
    let grant = fixture
        .client
        .in_folder_router_auto_for_test("loaded-session");
    fixture.client.shutdown().await;

    // Assert
    assert_eq!(
        mode_requests_for(&fixture.receipt(), "loaded-session"),
        vec!["default".to_owned()],
        "the restore sets the recorded mode back"
    );
    assert_eq!(grant, Some(false), "a named mode never holds Router auto");
    assert_eq!(loaded.permission_mode.requested.as_deref(), Some("default"));
    assert_eq!(loaded.permission_mode.actual.as_deref(), Some("default"));
}

/// Oracle: the Lead's V6 ruling, "if setting the mode fails, the session
/// still loads, but the grant stays off and show reports the mismatch (fail
/// closed)": a session recorded in auto that the agent restores in default
/// and refuses to switch loads with its grant off, its requested mode auto
/// and its actual mode default.
#[tokio::test]
async fn a_restore_whose_mode_set_fails_loads_without_a_grant_and_shows_the_mismatch() {
    // Arrange
    let fixture = Fixture::start(&["advertise-restore", "reject-mode"]).await;

    // Act
    let resumed = fixture
        .client
        .resume_session_with_setup(
            "resumed-session".to_owned(),
            fixture.project.clone(),
            fixture.restore_setup(Some("auto")),
        )
        .await
        .expect("the session still loads");
    let grant = fixture
        .client
        .in_folder_router_auto_for_test("resumed-session");
    fixture.client.shutdown().await;

    // Assert
    assert_eq!(
        mode_requests_for(&fixture.receipt(), "resumed-session"),
        vec!["auto".to_owned()],
        "the restore tried to set the recorded mode back"
    );
    assert_eq!(grant, Some(false), "a refused set leaves no allowance");
    assert_eq!(resumed.permission_mode.requested.as_deref(), Some("auto"));
    assert_eq!(resumed.permission_mode.actual.as_deref(), Some("default"));
}

/// Oracle: owner Decision 3, "old rows keep the fallback": a restore with no
/// recorded setup sends no additional directories and leaves no grant, so
/// every request asks, as before.
#[tokio::test]
async fn a_restore_without_a_recorded_setup_grants_nothing() {
    // Arrange
    let fixture = Fixture::start(&["advertise-restore", "restored-in-auto"]).await;

    // Act
    let resumed = fixture
        .client
        .resume_session_with_setup(
            "old-session".to_owned(),
            fixture.project.clone(),
            fixture.restore_setup(None),
        )
        .await
        .expect("resume");
    let grant = fixture.client.in_folder_router_auto_for_test("old-session");
    fixture.client.shutdown().await;

    // Assert
    let restore = fixture.receipt()["restores"][0].clone();
    assert!(
        matches!(&restore["additionalDirectories"], Value::Null)
            || restore["additionalDirectories"] == json!([]),
        "{restore}"
    );
    assert_eq!(grant, None);
    assert_eq!(resumed.permission_mode.requested, None);
}

/// Oracle: Lead V2 and owner Decision 3, "allowance follows the restored
/// mode": a session created in auto has an active grant; its run ends
/// without the connection ending; it is resumed in a named mode with a
/// scratch folder that no longer resolves, so the restore has no grant to
/// write. The earlier run's auto grant is gone: no grant is active, and an
/// in-folder edit goes to the approver instead of being allowed.
#[tokio::test]
async fn a_restore_without_a_grant_leaves_no_earlier_runs_allowance() {
    // Arrange: an active auto grant, then the run ends on its own.
    let fixture = Fixture::start(&["advertise-restore"]).await;
    let created = fixture.create("auto").await.expect("created in auto");
    let session = created.provider_session_id;
    let before = fixture.client.in_folder_router_auto_for_test(&session);
    fixture
        .client
        .end_session_actor(session.clone())
        .await
        .expect("the connection takes the command");
    while fixture
        .client
        .session_activity(session.clone())
        .await
        .expect("activity")
        != crate::ProviderSessionActivity::NotLoaded
    {
        tokio::task::yield_now().await;
    }

    // Act: resumed in a named mode; its scratch folder no longer resolves.
    let resumed = fixture
        .client
        .resume_session_with_setup(
            session.clone(),
            fixture.project.clone(),
            crate::SessionRestoreSetup {
                requested_mode: Some("default".to_owned()),
                scratch_folder: Some(fixture.project.join("removed-scratch")),
                work_roots: crate::SessionWorkRoots::default(),
            },
        )
        .await
        .expect("resume");
    let after = fixture.client.in_folder_router_auto_for_test(&session);
    let edit = fixture.prompt_in_folder_edit(&session).await;
    fixture.client.shutdown().await;

    // Assert
    assert_eq!(before, Some(true), "the auto start had an active grant");
    assert_eq!(
        resumed.permission_mode.requested.as_deref(),
        Some("default")
    );
    assert_eq!(after, None, "no grant is left for the resumed run");
    let answers = fixture.receipt()["permissionAnswers"].clone();
    let last = answers
        .as_array()
        .and_then(|answers| answers.last())
        .cloned()
        .unwrap_or(Value::Null);
    assert_ne!(
        last["answer"]["outcome"],
        json!({"outcome": "selected", "optionId": "allow"}),
        "the in-folder edit was allowed without asking: {last} ({edit:?})"
    );
    assert!(
        !last.is_null(),
        "the agent asked for the edit's permission: {answers}"
    );
}

/// Oracle: note §12.3 (R2-3) and the Lead's ruling, "a restore reapplies
/// it": a session created with fast kept off loses its actor and is
/// resumed; its agent reads fast on again. The client turns fast off ahead
/// of the session's next command, so the session keeps fast off.
#[tokio::test]
async fn a_restored_session_keeps_fast_off() {
    // Arrange: created with the Router's create default, which keeps fast off.
    let fixture = Fixture::start(&["offer-fast", "advertise-restore", "restore-fast-on"]).await;
    let created = fixture
        .client
        .create_session_with_settings(
            fixture.project.clone(),
            Some(fixture.scratch.clone()),
            RequestedProviderSettings {
                mode: Some("auto".to_owned()),
                fast: crate::FastRequest::OffIfOffered,
                ..RequestedProviderSettings::default()
            },
        )
        .await
        .expect("created");
    let session = created.provider_session_id;
    fixture
        .client
        .end_session_actor(session.clone())
        .await
        .expect("the connection takes the command");
    while fixture
        .client
        .session_activity(session.clone())
        .await
        .expect("activity")
        != crate::ProviderSessionActivity::NotLoaded
    {
        tokio::task::yield_now().await;
    }

    // Act: resumed; the agent reads fast on.
    fixture
        .client
        .resume_session_with_setup(
            session.clone(),
            fixture.project.clone(),
            fixture.restore_setup(Some("auto")),
        )
        .await
        .expect("resume");
    // The session's next command runs after anything queued ahead of it.
    let _idle = fixture.client.wait_session_idle(session.clone()).await;
    let fast = fixture
        .client
        .settings_catalog(&session)
        .map(|catalog| catalog.fast_observed());
    fixture.client.shutdown().await;

    // Assert
    let receipt = fixture.receipt();
    let requests = receipt["fastRequests"].clone();
    assert_eq!(
        requests,
        json!([
            {"sessionId": session, "value": "false"},
            {"sessionId": session, "value": "false"},
        ]),
        "fast is turned off at create and again after the restore: {receipt}"
    );
    assert_eq!(fast, Some(crate::FastObserved::Off));
}

/// An unoffered saved mode leaves the same provider session usable,
/// with no mode write and no Router-auto grant for the reported mode.
#[tokio::test]
async fn an_unoffered_saved_mode_still_accepts_input_on_the_restored_session() {
    let fixture = Fixture::start_in_profile(
        "claude-without-auto-or-accept-edits",
        &["advertise-restore", "restored-in-auto"],
    )
    .await;
    let id = "resumed-session";
    let loaded = fixture
        .client
        .resume_session_with_setup(
            id.to_owned(),
            fixture.project.clone(),
            fixture.restore_setup(Some("acceptEdits")),
        )
        .await
        .expect("the same session resumes");
    fixture
        .prompt_in_folder_edit(id)
        .await
        .expect("the same session accepts input");
    fixture
        .client
        .accept_session_settings(id.to_owned())
        .expect("accept is allowed after restore");
    fixture.client.shutdown().await;
    let receipt = fixture.receipt();
    assert_eq!(
        loaded.permission_mode.requested.as_deref(),
        Some("acceptEdits")
    );
    assert_eq!(loaded.permission_mode.actual.as_deref(), Some("auto"));
    assert!(mode_requests_for(&receipt, id).is_empty());
    assert_eq!(receipt["prompts"], json!([id]));
    assert_eq!(
        receipt["permissionAnswers"][0]["answer"]["outcome"]["outcome"],
        "cancelled"
    );
}
