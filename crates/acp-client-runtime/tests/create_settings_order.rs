//! A real ACP subprocess proves that create settings finish before any prompt.

#[path = "support/provider_behavior.rs"]
mod test_behavior;
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

#[derive(Default)]
struct SettingsEventSink(std::sync::Mutex<Vec<session_event_model::SessionSettings>>);

impl SessionEventSink for SettingsEventSink {
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
        if let SessionEvent::SettingsChanged { settings } = event {
            self.0.lock().expect("settings events").push(settings);
        }
        Ok(())
    }
}

struct ClosedSettingsSink;
impl SessionEventSink for ClosedSettingsSink {
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
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
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
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
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
        .submit_with_approval_dispatch_for_input(
            created.provider_session_id,
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Proceed".to_owned())
                        .expect("text prompt"),
                ]),
            ),
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
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
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
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
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
        .submit_with_approval_dispatch_for_input(
            provider_session_id.clone(),
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("too early".to_owned())
                        .expect("text prompt"),
                ]),
            ),
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
        .submit_with_approval_dispatch_for_input(
            provider_session_id,
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Proceed".to_owned())
                        .expect("text prompt"),
                ]),
            ),
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
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
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
            acp_client_runtime::ProviderSettingRequest {
                kind: acp_client_runtime::ProviderSettingKind::Model,
                value: "b".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
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
        .submit_with_approval_dispatch_for_input(
            provider_session_id,
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Proceed".to_owned())
                        .expect("text prompt"),
                ]),
            ),
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
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
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
        .submit_with_approval_dispatch_for_input(
            created.provider_session_id,
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Proceed".to_owned())
                        .expect("text prompt"),
                ]),
            ),
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

#[path = "create_settings_order/settings_tail_tests.rs"]
mod settings_tail_tests;
