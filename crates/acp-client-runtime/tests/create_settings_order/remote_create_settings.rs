//! R16 keeps a created remote Session visible even when its first choice is invalid.
use super::{NoopEventSink, NoopInteractionPort};
use acp_client_runtime::{
    AgentSessionClient, ExternalProviderLaunch, ExternalProviderRuntimeError,
    NotAppliedProviderSetting, ProviderPersistenceTarget, ProviderSettingKind,
    RequestedProviderSettings,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-remote-create-settings-agent.ts"
);

#[tokio::test]
async fn invalid_first_model_or_effort_keeps_the_known_created_session() {
    for kind in [ProviderSettingKind::Model, ProviderSettingKind::Effort] {
        let root = tempfile::tempdir().expect("provider fixture root");
        let receipt = root.path().join("exchange.json");
        let client = AgentSessionClient::initialize(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments: vec![FIXTURE.to_owned(), receipt.to_string_lossy().into_owned()],
                environment: Vec::new(),
                persistence_target: ProviderPersistenceTarget::Unspecified,
            },
            Arc::new(NoopInteractionPort),
            Arc::new(NoopEventSink),
        )
        .await
        .expect("provider initializes");
        let requested = match kind {
            ProviderSettingKind::Model => RequestedProviderSettings {
                model: Some("missing-model".to_owned()),
                effort: Some("high".to_owned()),
                mode: None,
            },
            _ => RequestedProviderSettings {
                effort: Some("missing-effort".to_owned()),
                ..Default::default()
            },
        };

        let result = tokio::time::timeout(
            Duration::from_secs(10),
            client.create_session_retaining_partial_settings(root.path().to_path_buf(), requested),
        )
        .await
        .expect("create settles");
        let Err(ExternalProviderRuntimeError::CreatedWithoutSettings {
            provider_session_id,
            applied,
            failed,
            not_applied,
        }) = result
        else {
            client.shutdown().await;
            panic!("created session and partial details are required: {result:?}")
        };
        assert_eq!(provider_session_id, "remote-created-session");
        assert!(applied.is_empty());
        assert_eq!(failed.kind, kind);
        assert_eq!(
            failed.reason,
            session_event_model::ProviderSettingFailureReason::InvalidSetting
        );
        assert_eq!(
            failed.value,
            if kind == ProviderSettingKind::Model {
                "missing-model"
            } else {
                "missing-effort"
            }
        );
        let expected_untried = if kind == ProviderSettingKind::Model {
            vec![NotAppliedProviderSetting {
                kind: ProviderSettingKind::Effort,
                value: "high".to_owned(),
            }]
        } else {
            Vec::new()
        };
        assert_eq!(not_applied.as_ref(), expected_untried);
        assert!(client.settings_unresolved(&provider_session_id).await);
        client
            .accept_session_settings(provider_session_id.clone())
            .await
            .expect("accept effective settings on the retained session");
        let prompt = client
            .prompt_contents_with_approval_dispatch_for_input(
                provider_session_id,
                session_event_model::InputId::generate(),
                vec![
                    session_event_model::PromptContent::text("fixture input".to_owned())
                        .expect("fixture text"),
                ],
                (),
                None,
            )
            .await;
        client.shutdown().await;
        assert!(
            prompt.is_ok(),
            "retained session can receive after acceptance: {prompt:?}"
        );
        assert!(!client.shutdown_failed());
        let methods: Vec<String> = serde_json::from_slice(
            &std::fs::read(&receipt).expect("closed-producer exchange receipt"),
        )
        .expect("exchange JSON");
        assert_eq!(
            methods
                .iter()
                .filter(|method| method.as_str() == "session/new")
                .count(),
            1
        );
        assert_eq!(
            methods
                .iter()
                .filter(|method| method.as_str() == "session/close")
                .count(),
            0
        );
        assert_eq!(
            methods
                .iter()
                .filter(|method| method.as_str() == "session/set_config_option")
                .count(),
            0
        );
        assert_eq!(
            methods
                .iter()
                .filter(|method| method.as_str() == "session/prompt")
                .count(),
            1
        );
    }
}
