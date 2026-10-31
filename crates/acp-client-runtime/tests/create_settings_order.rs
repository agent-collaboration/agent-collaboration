//! A real ACP subprocess proves that create settings finish before any prompt.

use std::{path::PathBuf, sync::Arc};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderPersistenceTarget,
    RefusedApprovalOffer, RequestedProviderSettings, SessionEventSink,
};
use session_event_model::{ApprovalRequest, SessionEvent};
use tokio_util::sync::CancellationToken;

const SETTINGS_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-create-settings-agent.ts"
);

const FAILED_SETTINGS_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-failed-settings-agent.ts"
);

const LEGACY_MODE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-legacy-mode-agent.ts"
);

const SETTING_RESPONSE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-setting-response-agent.ts"
);

const HELD_SETTING_RESPONSE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-setting-response-agent.ts"
);

const SILENT_SETTING_RESPONSE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-setting-response-agent.ts"
);

const LEGACY_SETTING_RESPONSE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-setting-response-agent.ts"
);

const CLOSED_SETTINGS_SINK_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-setting-response-agent.ts"
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
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(&self, _session_id: &str, _event: SessionEvent) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

#[derive(Default)]
struct SettingsEventSink(std::sync::Mutex<Vec<session_event_model::SessionSettings>>);

impl SessionEventSink for SettingsEventSink {
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
        if let SessionEvent::SettingsChanged { settings } = event {
            self.0.lock().expect("settings events").push(settings);
        }
        Ok(())
    }
}

struct ClosedSettingsSink;
impl SessionEventSink for ClosedSettingsSink {
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(&self, _session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        if matches!(event, SessionEvent::SettingsChanged { .. }) {
            Err(EventSinkClosed)
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn closed_settings_sink_aborts_create_with_typed_error() {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![CLOSED_SETTINGS_SINK_FIXTURE.to_owned(), "closed".to_owned()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::new(ClosedSettingsSink),
    )
    .await
    .expect("fixture initializes");
    let result = client.create_session(root.path().to_path_buf()).await;
    assert!(matches!(
        result,
        Err(acp_client_runtime::ExternalProviderRuntimeError::SinkClosed)
    ));
    client.shutdown().await;
}

/// Oracle: specification R14 requires mode, model and effort to be applied
/// after session/new and before the first prompt, re-reading options each time.
#[tokio::test]
async fn create_applies_requested_settings_before_first_prompt() {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("exchange.json");
    let sink = Arc::new(SettingsEventSink::default());
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                SETTINGS_FIXTURE.to_owned(),
                receipt.to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        sink.clone(),
    )
    .await
    .expect("fixture initializes");

    let created = client
        .create_session_with_settings(
            root.path().to_path_buf(),
            RequestedProviderSettings {
                mode: Some("ask".to_owned()),
                model: Some("b".to_owned()),
                effort: Some("high".to_owned()),
            },
        )
        .await
        .expect("settings applied");
    assert_eq!(created.effective_settings.mode.as_deref(), Some("ask"));
    assert_eq!(created.effective_settings.model.as_deref(), Some("b"));
    assert_eq!(created.effective_settings.effort.as_deref(), Some("high"));
    {
        let settings = sink.0.lock().expect("settings events");
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].mode.as_deref(), Some("ask"));
        assert_eq!(settings[0].model.as_deref(), Some("b"));
        assert_eq!(settings[0].effort.as_deref(), Some("high"));
    }

    let prompt = client
        .prompt_contents_with_approval_dispatch_for_input(
            created.provider_session_id,
            session_event_model::InputId::generate(),
            vec![
                session_event_model::PromptContent::text("Proceed".to_owned())
                    .expect("text prompt"),
            ],
            (),
            None,
        )
        .await;
    client.shutdown().await;
    assert!(prompt.is_ok(), "prompt result: {prompt:?}");
    let seen: Vec<String> =
        serde_json::from_slice(&std::fs::read(receipt).expect("receipt")).expect("receipt JSON");
    assert_eq!(seen, ["mode", "model", "effort", "prompt"]);
}

fn failure_fixture_launch(mode: &str, receipt: &std::path::Path) -> ExternalProviderLaunch {
    ExternalProviderLaunch {
        executable: PathBuf::from("node"),
        arguments: vec![
            FAILED_SETTINGS_FIXTURE.to_owned(),
            mode.to_owned(),
            receipt.to_string_lossy().into_owned(),
        ],
        environment: Vec::new(),
        persistence_target: ProviderPersistenceTarget::Unspecified,
    }
}

/// Oracle: specification R14 rejects an unoffered value with advertised
/// choices and closes the newly created Session when close is supported.
#[tokio::test]
async fn invalid_first_setting_closes_created_session() {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("closed.txt");
    let client = AgentSessionClient::initialize(
        failure_fixture_launch("invalid", &receipt),
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
    )
    .await
    .expect("fixture initializes");
    let result = client
        .create_session_with_settings(
            root.path().to_path_buf(),
            RequestedProviderSettings {
                mode: Some("impossible".to_owned()),
                ..Default::default()
            },
        )
        .await;
    client.shutdown().await;
    let Err(acp_client_runtime::ExternalProviderRuntimeError::InvalidSetting {
        setting,
        value,
        advertised,
        disposition,
        ..
    }) = result
    else {
        panic!(
            "invalid setting result: {result:?}; close receipt: {:?}",
            std::fs::read_to_string(&receipt)
        )
    };
    assert_eq!(setting, acp_client_runtime::ProviderSettingKind::Mode);
    assert_eq!(value, "impossible");
    assert_eq!(advertised, ["auto", "ask"]);
    assert_eq!(
        disposition,
        acp_client_runtime::InvalidSettingSessionDisposition::Closed
    );
    assert_eq!(
        std::fs::read_to_string(receipt).expect("close receipt"),
        "closed"
    );
}

/// Oracle: specification R14 reports a partly applied setup and blocks work
/// until the caller explicitly accepts the effective settings.
#[tokio::test]
async fn partial_setup_blocks_prompt_until_settings_are_accepted() {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("accepted.txt");
    let client = AgentSessionClient::initialize(
        failure_fixture_launch("partial", &receipt),
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
    )
    .await
    .expect("fixture initializes");
    let result = client
        .create_session_with_settings(
            root.path().to_path_buf(),
            RequestedProviderSettings {
                mode: Some("ask".to_owned()),
                model: Some("b".to_owned()),
                effort: Some("high".to_owned()),
            },
        )
        .await;
    let Err(acp_client_runtime::ExternalProviderRuntimeError::CreatedWithoutSettings {
        provider_session_id,
        applied,
        failed,
        not_applied,
    }) = result
    else {
        panic!("partial setup result: {result:?}")
    };
    assert_eq!(provider_session_id, "fixture-session");
    assert_eq!(applied.len(), 1);
    assert_eq!(
        applied[0].kind,
        acp_client_runtime::ProviderSettingKind::Mode
    );
    assert_eq!(failed.kind, acp_client_runtime::ProviderSettingKind::Model);
    assert!(!failed.reason.as_str().contains("private agent detail"));
    assert_eq!(
        not_applied,
        vec![acp_client_runtime::NotAppliedProviderSetting {
            kind: acp_client_runtime::ProviderSettingKind::Effort,
            value: "high".to_owned(),
        }]
        .into_boxed_slice()
    );
    assert!(client.settings_unresolved(&provider_session_id).await);
    let blocked = client
        .prompt_contents_with_approval_dispatch_for_input(
            provider_session_id.clone(),
            session_event_model::InputId::generate(),
            vec![
                session_event_model::PromptContent::text("too early".to_owned())
                    .expect("text prompt"),
            ],
            (),
            None,
        )
        .await;
    assert!(matches!(
        blocked,
        Err(acp_client_runtime::ExternalProviderRuntimeError::SettingsUnresolved)
    ));
    client
        .accept_session_settings(provider_session_id.clone())
        .await
        .expect("caller accepts effective settings");
    let prompt = client
        .prompt_contents_with_approval_dispatch_for_input(
            provider_session_id,
            session_event_model::InputId::generate(),
            vec![
                session_event_model::PromptContent::text("Proceed".to_owned())
                    .expect("text prompt"),
            ],
            (),
            None,
        )
        .await;
    client.shutdown().await;
    assert!(prompt.is_ok(), "prompt after accept: {prompt:?}");
    assert_eq!(
        std::fs::read_to_string(receipt).expect("prompt receipt"),
        "prompt-after-accept"
    );
}

/// Oracle: specification R14 lets the caller resolve a partial setup by
/// applying a currently offered value before prompting.
#[tokio::test]
async fn resolving_failed_kind_keeps_untried_kind_gated_until_accepted() {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("resolved.txt");
    let sink = Arc::new(SettingsEventSink::default());
    let client = AgentSessionClient::initialize(
        failure_fixture_launch("partial-set", &receipt),
        Arc::new(NoopInteractionPort),
        sink.clone(),
    )
    .await
    .expect("fixture initializes");
    let created = client
        .create_session_with_settings(
            root.path().to_path_buf(),
            RequestedProviderSettings {
                mode: Some("ask".to_owned()),
                model: Some("b".to_owned()),
                effort: Some("high".to_owned()),
            },
        )
        .await;
    let Err(acp_client_runtime::ExternalProviderRuntimeError::CreatedWithoutSettings {
        provider_session_id,
        ..
    }) = created
    else {
        panic!("partial setup result: {created:?}")
    };
    let effective = client
        .set_setting(
            provider_session_id.clone(),
            acp_client_runtime::ProviderSettingKind::Model,
            "b".to_owned(),
        )
        .await
        .expect("model setting applied");
    assert_eq!(effective.mode.as_deref(), Some("ask"));
    assert_eq!(effective.model.as_deref(), Some("b"));
    {
        let settings = sink.0.lock().expect("settings events");
        assert_eq!(settings.len(), 2, "create and set_setting each publish");
        assert_eq!(settings[0].mode.as_deref(), Some("ask"));
        assert_eq!(settings[0].model.as_deref(), Some("a"));
        assert_eq!(settings[1].model.as_deref(), Some("b"));
    }
    assert!(client.settings_unresolved(&provider_session_id).await);
    client
        .accept_session_settings(provider_session_id.clone())
        .await
        .expect("accept remaining untried setting");
    assert!(!client.settings_unresolved(&provider_session_id).await);
    let prompt = client
        .prompt_contents_with_approval_dispatch_for_input(
            provider_session_id,
            session_event_model::InputId::generate(),
            vec![
                session_event_model::PromptContent::text("Proceed".to_owned())
                    .expect("text prompt"),
            ],
            (),
            None,
        )
        .await;
    client.shutdown().await;
    assert!(prompt.is_ok(), "prompt after setting: {prompt:?}");
    assert_eq!(
        std::fs::read_to_string(receipt).expect("prompt receipt"),
        "prompt-after-accept"
    );
}

/// Oracle: ACP v1 session modes are the fallback only when no mode config
/// option is offered (specification R14; session-modes.mdx).
#[tokio::test]
async fn legacy_mode_is_selected_before_prompt_when_no_mode_config_exists() {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("legacy-mode.txt");
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                LEGACY_MODE_FIXTURE.to_owned(),
                receipt.to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
    )
    .await
    .expect("fixture initializes");
    let created = client
        .create_session_with_settings(
            root.path().to_path_buf(),
            RequestedProviderSettings {
                mode: Some("ask".to_owned()),
                ..Default::default()
            },
        )
        .await
        .expect("mode applied");
    assert_eq!(created.effective_settings.mode.as_deref(), Some("ask"));
    let prompt = client
        .prompt_contents_with_approval_dispatch_for_input(
            created.provider_session_id,
            session_event_model::InputId::generate(),
            vec![
                session_event_model::PromptContent::text("Proceed".to_owned())
                    .expect("text prompt"),
            ],
            (),
            None,
        )
        .await;
    client.shutdown().await;
    assert!(prompt.is_ok(), "prompt result: {prompt:?}");
    assert_eq!(
        std::fs::read_to_string(receipt).expect("receipt"),
        "mode-before-prompt"
    );
}

fn setting_response_launch(mode: &str) -> ExternalProviderLaunch {
    ExternalProviderLaunch {
        executable: PathBuf::from("node"),
        arguments: vec![SETTING_RESPONSE_FIXTURE.to_owned(), mode.to_owned()],
        environment: Vec::new(),
        persistence_target: ProviderPersistenceTarget::Unspecified,
    }
}

/// Oracle: once a setting request was sent, an unusable success response
/// leaves the effect unknown. Work stays gated until the caller resolves it.
#[tokio::test]
async fn queued_prompt_is_gated_after_in_flight_setting_becomes_uncertain() {
    let root = tempfile::tempdir().expect("fixture root");
    let ready_path = root.path().join("setting-ready.fifo");
    let go_path = root.path().join("setting-go.fifo");
    for path in [&ready_path, &go_path] {
        assert!(
            std::process::Command::new("mkfifo")
                .arg(path)
                .status()
                .expect("create FIFO")
                .success()
        );
    }
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                HELD_SETTING_RESPONSE_FIXTURE.to_owned(),
                "held".to_owned(),
                ready_path.to_string_lossy().into_owned(),
                go_path.to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let setting = client.set_setting(
        session_id.clone(),
        acp_client_runtime::ProviderSettingKind::Mode,
        "ask".to_owned(),
    );
    tokio::pin!(setting);
    assert!(futures_util::poll!(setting.as_mut()).is_pending());
    let ready = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut barrier = std::fs::OpenOptions::new()
            .read(true)
            .open(ready_path)
            .expect("setting reached agent");
        let mut ready = [0_u8; 1];
        barrier.read_exact(&mut ready).expect("setting held");
        ready
    })
    .await
    .expect("barrier reader");
    assert_eq!(ready, [b'R']);

    let prompt = client.prompt_contents_with_approval_dispatch_for_input(
        session_id.clone(),
        session_event_model::InputId::generate(),
        vec![
            session_event_model::PromptContent::text("Do not dispatch".to_owned())
                .expect("text prompt"),
        ],
        (),
        None,
    );
    tokio::pin!(prompt);
    assert!(futures_util::poll!(prompt.as_mut()).is_pending());
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut release = std::fs::OpenOptions::new()
            .write(true)
            .open(go_path)
            .expect("release barrier");
        release.write_all(b"G").expect("release setting");
    })
    .await
    .expect("barrier writer");

    assert!(matches!(
        setting.await,
        Err(acp_client_runtime::ExternalProviderRuntimeError::SettingOutcomeUnknown { .. })
    ));
    assert!(matches!(
        prompt.await,
        Err(acp_client_runtime::ExternalProviderRuntimeError::SettingsUnresolved)
    ));
    client.shutdown().await;
}

#[tokio::test]
async fn silent_setting_response_expires_to_unknown_and_gates_session() {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                SILENT_SETTING_RESPONSE_FIXTURE.to_owned(),
                "silent".to_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    tokio::time::pause();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(32),
        client.set_setting(
            session_id.clone(),
            acp_client_runtime::ProviderSettingKind::Mode,
            "ask".to_owned(),
        ),
    )
    .await;
    tokio::time::resume();
    if result.is_err() {
        client.shutdown().await;
        panic!("setting request remained busy beyond its deadline");
    }
    assert!(matches!(
        result,
        Ok(Err(
            acp_client_runtime::ExternalProviderRuntimeError::SettingOutcomeUnknown { .. }
        ))
    ));
    assert!(client.settings_unresolved(&session_id).await);
    let blocked = client
        .prompt_contents_with_approval_dispatch_for_input(
            session_id,
            session_event_model::InputId::generate(),
            vec![
                session_event_model::PromptContent::text("Must stay gated".to_owned())
                    .expect("text prompt"),
            ],
            (),
            None,
        )
        .await;
    assert!(matches!(
        blocked,
        Err(acp_client_runtime::ExternalProviderRuntimeError::SettingsUnresolved)
    ));
    client.shutdown().await;
}

/// Oracle: once a setting request was sent, an unusable success response
/// leaves the effect unknown. Work stays gated until the caller resolves it.
#[tokio::test]
async fn malformed_post_send_setting_response_gates_existing_session() {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        setting_response_launch("malformed"),
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let result = client
        .set_setting(
            session_id.clone(),
            acp_client_runtime::ProviderSettingKind::Mode,
            "ask".to_owned(),
        )
        .await;
    assert!(
        matches!(&result, Err(acp_client_runtime::ExternalProviderRuntimeError::SettingOutcomeUnknown {
        provider_session_id, setting: acp_client_runtime::ProviderSettingKind::Mode, value,
    }) if provider_session_id == &session_id && value == "ask"),
        "setting result: {result:?}"
    );
    assert!(client.settings_unresolved(&session_id).await);
    assert!(matches!(
        client.accept_session_settings(session_id.clone()).await,
        Err(acp_client_runtime::ExternalProviderRuntimeError::SettingsOutcomeUncertain)
    ));
    let blocked = client
        .prompt_contents_with_approval_dispatch_for_input(
            session_id,
            session_event_model::InputId::generate(),
            vec![
                session_event_model::PromptContent::text("Must not run".to_owned())
                    .expect("text prompt"),
            ],
            (),
            None,
        )
        .await;
    assert!(matches!(
        blocked,
        Err(acp_client_runtime::ExternalProviderRuntimeError::SettingsUnresolved)
    ));
    client.shutdown().await;
}

#[path = "create_settings_order/settings_tail_tests.rs"]
mod settings_tail_tests;
