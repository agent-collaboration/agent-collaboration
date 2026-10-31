use super::*;

/// An explicit JSON-RPC rejection is definite: the old setting remains
/// effective and a later prompt may proceed.
#[tokio::test]
async fn explicit_setting_rejection_does_not_gate_session() {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        setting_response_launch("rejected"),
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
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
            acp_client_runtime::ProviderSettingRequest {
                kind: acp_client_runtime::ProviderSettingKind::Mode,
                value: "ask".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await;
    assert!(
        matches!(
            result,
            Err(acp_client_runtime::ExternalProviderRuntimeError::SettingFailed { .. })
        ),
        "setting result: {result:?}"
    );
    assert!(!client.settings_unresolved(&session_id).await);
    client
        .submit_with_approval_dispatch_for_input(
            session_id,
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
        .await
        .expect("prompt after rejection");
    client.shutdown().await;
}

#[tokio::test]
async fn malformed_legacy_set_mode_response_is_outcome_unknown() {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                LEGACY_SETTING_RESPONSE_FIXTURE.to_owned(),
                "legacy".to_owned(),
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
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let result = client
        .set_setting(
            session_id.clone(),
            acp_client_runtime::ProviderSettingRequest {
                kind: acp_client_runtime::ProviderSettingKind::Mode,
                value: "ask".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await;
    assert!(
        matches!(
            result,
            Err(acp_client_runtime::ExternalProviderRuntimeError::SettingOutcomeUnknown { .. })
        ),
        "legacy setting result: {result:?}"
    );
    assert!(client.settings_unresolved(&session_id).await);
    client.shutdown().await;
}

#[tokio::test]
async fn disconnected_after_setting_submission_is_outcome_unknown() {
    let root = tempfile::tempdir().expect("fixture root");
    let client = AgentSessionClient::initialize(
        setting_response_launch("disconnect"),
        Arc::new(NoopInteractionPort),
        Arc::new(NoopEventSink),
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.set_setting(
            session_id.clone(),
            acp_client_runtime::ProviderSettingRequest {
                kind: acp_client_runtime::ProviderSettingKind::Mode,
                value: "ask".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        ),
    )
    .await
    .expect("setting result bounded");
    assert!(
        matches!(
            result,
            Err(acp_client_runtime::ExternalProviderRuntimeError::SettingOutcomeUnknown { .. })
        ),
        "disconnected setting result: {result:?}"
    );
    assert!(client.settings_unresolved(&session_id).await);
    client.shutdown().await;
}

use acp_client_runtime::{
    PreparedCompact, ProviderBehavior, ProviderError, ProviderFacts, ProviderUpdate,
};
use session_event_model::{PromptContent, ProviderSubmission};
/// A peer whose setting state cannot be inferred from one Session's cache.
struct NonReusableTestBehavior;
impl ProviderBehavior for NonReusableTestBehavior {
    fn can_reuse_setting_confirmation(&self, _: acp_client_runtime::ProviderSettingKind) -> bool {
        false
    }
    fn model_metadata_source(&self) -> acp_client_runtime::ModelMetadataSource {
        test_behavior::TestProviderBehavior.model_metadata_source()
    }
    fn decode_model_metadata(
        &self,
        value: &serde_json::Value,
    ) -> acp_client_runtime::ModelReasoningMetadata {
        test_behavior::TestProviderBehavior.decode_model_metadata(value)
    }
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        test_behavior::TestProviderBehavior.observe(update)
    }
    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError> {
        test_behavior::TestProviderBehavior.prepare(submission)
    }
    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        test_behavior::TestProviderBehavior.compact()
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
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let setting = client.set_setting(
        session_id.clone(),
        acp_client_runtime::ProviderSettingRequest {
            kind: acp_client_runtime::ProviderSettingKind::Mode,
            value: "ask".to_owned(),
            intent: session_event_model::SettingApplicationIntent::ExplicitChange,
            expected_model: None,
        },
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

    let prompt = client.submit_with_approval_dispatch_for_input(
        session_id.clone(),
        session_event_model::InputId::generate(),
        acp_client_runtime::ProviderTurnInput::Prompt(
            session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::text("Do not dispatch".to_owned())
                    .expect("text prompt"),
            ]),
        ),
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
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    let session_id = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(32),
        client.set_setting(
            session_id.clone(),
            acp_client_runtime::ProviderSettingRequest {
                kind: acp_client_runtime::ProviderSettingKind::Mode,
                value: "ask".to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        ),
    )
    .await;
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
        .submit_with_approval_dispatch_for_input(
            session_id,
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Must stay gated".to_owned())
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
    client.shutdown().await;
}

/// Oracle: once a setting request was sent, an unusable success response
/// leaves the effect unknown. Work stays gated until the caller resolves it.
#[tokio::test]
async fn malformed_post_send_setting_response_gates_existing_session() {
    let behaviors: [Arc<dyn ProviderBehavior>; 2] = [
        Arc::new(test_behavior::TestProviderBehavior),
        Arc::new(NonReusableTestBehavior),
    ];
    for behavior in behaviors {
        let root = tempfile::tempdir().expect("fixture root");
        let client = AgentSessionClient::initialize(
            setting_response_launch("malformed"),
            Arc::new(NoopInteractionPort),
            Arc::new(NoopEventSink),
            behavior,
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
                acp_client_runtime::ProviderSettingRequest {
                    kind: acp_client_runtime::ProviderSettingKind::Mode,
                    value: "ask".to_owned(),
                    intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                    expected_model: None,
                },
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
        for value in ["auto", "ask"] {
            let implicit = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                client.set_setting(
                    session_id.clone(),
                    acp_client_runtime::ProviderSettingRequest {
                        kind: acp_client_runtime::ProviderSettingKind::Mode,
                        value: value.into(),
                        intent: session_event_model::SettingApplicationIntent::TurnContext,
                        expected_model: None,
                    },
                ),
            )
            .await
            .expect("implicit context refused without waiting for peer");
            assert!(matches!(
                implicit,
                Err(acp_client_runtime::ExternalProviderRuntimeError::SettingsUnresolved)
            ));
        }
        let blocked = client
            .submit_with_approval_dispatch_for_input(
                session_id,
                session_event_model::InputId::generate(),
                acp_client_runtime::ProviderTurnInput::Prompt(
                    session_event_model::ProviderSubmission::plain(vec![
                        session_event_model::PromptContent::text("Must not run".to_owned())
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
        client.shutdown().await;
    }
}
