//! R1–R4: real ACP wires and returned catalogs, without native provider state.

#[path = "cursor_model_settings/acp_wire_fixture.rs"]
mod acp_wire_fixture;
#[path = "support/provider_behavior.rs"]
mod test_behavior;

use acp_client_runtime::{
    ExternalProviderRuntimeError, ProviderConfigValue, ProviderSettingKind,
    RequestedProviderSettings,
};
use acp_wire_fixture::AcpWireFixture;
use session_event_model::ConfigValueState;

async fn assert_shared_setting_is_reasserted(
    kind: ProviderSettingKind,
    initial: &str,
    changed: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use acp_client_runtime::ProviderSettingRequest;
    use session_event_model::SettingApplicationIntent;
    let fixture = AcpWireFixture::start("shared-settings", false).await;
    let first = fixture
        .client
        .create_session(fixture.root.path().into())
        .await?;
    let second = fixture
        .client
        .create_session(fixture.root.path().into())
        .await?;
    assert_ne!(first, second);
    let request = |value: &str, intent| ProviderSettingRequest {
        kind,
        value: value.into(),
        intent,
        expected_model: None,
    };
    let confirmed = fixture
        .client
        .set_setting(
            first.clone(),
            request(initial, SettingApplicationIntent::ExplicitChange),
        )
        .await?;
    let other = fixture
        .client
        .set_setting(
            second.clone(),
            request(changed, SettingApplicationIntent::ExplicitChange),
        )
        .await?;
    let reasserted = fixture
        .client
        .set_setting(
            first.clone(),
            request(initial, SettingApplicationIntent::TurnContext),
        )
        .await?;
    fixture.client.shutdown().await;
    let value = |settings: acp_client_runtime::EffectiveProviderSettings| match kind {
        ProviderSettingKind::Model => settings.model,
        ProviderSettingKind::Effort => settings.effort,
        ProviderSettingKind::Mode => settings.mode,
    };
    assert_eq!(value(confirmed).as_deref(), Some(initial));
    assert_eq!(value(other).as_deref(), Some(changed));
    assert_eq!(value(reasserted).as_deref(), Some(initial));
    let config_id = match kind {
        ProviderSettingKind::Model => "model",
        ProviderSettingKind::Effort => "effort",
        ProviderSettingKind::Mode => "mode",
    };
    let setters = fixture
        .requests()
        .into_iter()
        .filter(|request| {
            request.get("method").and_then(serde_json::Value::as_str)
                == Some("session/set_config_option")
        })
        .map(|request| {
            request
                .get("params")
                .cloned()
                .ok_or("setter parameters missing")
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        setters,
        vec![
            serde_json::json!({"sessionId":first,"configId":config_id,"value":initial}),
            serde_json::json!({"sessionId":second,"configId":config_id,"value":changed}),
            serde_json::json!({"sessionId":first,"configId":config_id,"value":initial}),
        ],
        "A's stale equality must not skip the actual shared-setting setter"
    );
    Ok(())
}

#[tokio::test]
async fn nonreusable_model_confirmation_reasserts_after_another_session_changes_shared_state()
-> Result<(), Box<dyn std::error::Error>> {
    assert_shared_setting_is_reasserted(ProviderSettingKind::Model, "fixture-a", "fixture-b").await
}

#[tokio::test]
async fn nonreusable_effort_confirmation_reasserts_after_another_session_changes_shared_state()
-> Result<(), Box<dyn std::error::Error>> {
    assert_shared_setting_is_reasserted(ProviderSettingKind::Effort, "medium", "high").await
}

#[tokio::test]
async fn repeated_turn_context_preserves_confirmed_effort_without_setting_wire() {
    let fixture = AcpWireFixture::start("thinking-first", false).await;
    let session = fixture
        .client
        .create_session(fixture.root.path().into())
        .await
        .expect("session");
    let initial_events = fixture.sink.settings.lock().expect("settings").len();
    let result = fixture
        .client
        .set_setting(
            session.clone(),
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Effort,
                value: "medium".into(),
                intent: session_event_model::SettingApplicationIntent::TurnContext,
                expected_model: None,
            },
        )
        .await;
    let second = fixture
        .client
        .set_setting(
            session.clone(),
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Effort,
                value: "medium".into(),
                intent: session_event_model::SettingApplicationIntent::TurnContext,
                expected_model: None,
            },
        )
        .await;
    fixture.client.shutdown().await;
    let settings = fixture
        .requests()
        .into_iter()
        .filter(|request| request["method"] == "session/set_config_option")
        .collect::<Vec<_>>();
    assert!(
        settings.is_empty(),
        "unchanged context must not re-pin settings: {settings:?}"
    );
    assert_eq!(
        result.expect("confirmed context").effort.as_deref(),
        Some("medium")
    );
    assert_eq!(
        second.expect("second context").effort.as_deref(),
        Some("medium")
    );
    assert_eq!(
        fixture.sink.settings.lock().expect("settings").len(),
        initial_events,
        "no-op must publish no new SettingsChanged event"
    );
}

#[tokio::test]
async fn effort_context_guard_rejects_a_different_confirmed_model_before_any_write() {
    let fixture = AcpWireFixture::start("thinking-first", false).await;
    let session = fixture
        .client
        .create_session(fixture.root.path().into())
        .await
        .expect("session");
    for intent in [
        session_event_model::SettingApplicationIntent::TurnContext,
        session_event_model::SettingApplicationIntent::ExplicitChange,
    ] {
        let result = fixture
            .client
            .set_setting(
                session.clone(),
                acp_client_runtime::ProviderSettingRequest {
                    kind: ProviderSettingKind::Effort,
                    value: "high".into(),
                    intent,
                    expected_model: Some("fixture-b".into()),
                },
            )
            .await;
        assert!(matches!(
            result,
            Err(ExternalProviderRuntimeError::SettingsContextChanged)
        ));
    }
    fixture.client.shutdown().await;
    assert!(
        fixture
            .requests()
            .iter()
            .all(|request| request["method"] != "session/set_config_option")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn overlapping_model_change_is_confirmed_before_dependent_effort_checks_expected_model() {
    use acp_client_runtime::ProviderSettingRequest;
    use session_event_model::SettingApplicationIntent;
    let fixture = AcpWireFixture::start("dynamic", false).await;
    let session = fixture
        .client
        .create_session(fixture.root.path().into())
        .await
        .expect("session");
    fixture
        .client
        .set_setting(
            session.clone(),
            ProviderSettingRequest {
                kind: ProviderSettingKind::Mode,
                value: "ask".into(),
                intent: SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
        .expect("fresh catalogue offers both models");
    let change = fixture.client.set_setting(
        session.clone(),
        ProviderSettingRequest {
            kind: ProviderSettingKind::Model,
            value: "fixture-b".into(),
            intent: SettingApplicationIntent::ExplicitChange,
            expected_model: None,
        },
    );
    let effort = fixture.client.set_setting(
        session.clone(),
        ProviderSettingRequest {
            kind: ProviderSettingKind::Effort,
            value: "high".into(),
            intent: SettingApplicationIntent::TurnContext,
            expected_model: Some("fixture-a".into()),
        },
    );
    tokio::pin!(change, effort);
    // On this single-thread executor both callers enter before the connection
    // task can settle either. The actor must check after the queued model effect.
    assert!(futures_util::poll!(change.as_mut()).is_pending());
    assert!(futures_util::poll!(effort.as_mut()).is_pending());
    let (changed, refused) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(change, effort)
    })
    .await
    .expect("actor settles both queued commands");
    assert_eq!(
        changed.expect("model confirmed").model.as_deref(),
        Some("fixture-b")
    );
    assert!(matches!(
        refused,
        Err(ExternalProviderRuntimeError::SettingsContextChanged)
    ));
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "session/set_config_option")
            .count(),
        2,
        "guarded effort must have no wire request"
    );
    let current = fixture
        .client
        .set_setting(
            session,
            ProviderSettingRequest {
                kind: ProviderSettingKind::Effort,
                value: "high".into(),
                intent: SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
        .expect("an intentional current-model setting remains supported");
    assert_eq!(current.effort.as_deref(), Some("high"));
    fixture.client.shutdown().await;
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "session/set_config_option")
            .count(),
        3,
        "positive control reaches the same peer's effort setter"
    );
}

#[cfg(feature = "test-observation")]
#[tokio::test]
async fn native_observation_entrypoint_uses_picker_wire_without_mcp_injection() {
    let fixture = AcpWireFixture::start_without_mcp("thinking-first").await;
    fixture
        .client
        .create_session(fixture.root.path().into())
        .await
        .expect("native-path fixture session");
    fixture.client.shutdown().await;
    let requests = fixture.requests();
    assert_eq!(
        requests[0]["params"]["clientCapabilities"]["_meta"]["parameterizedModelPicker"],
        true
    );
    assert_eq!(requests[1]["params"]["mcpServers"], serde_json::json!([]));
}

#[tokio::test]
async fn cursor_initialize_advertises_parameterized_picker_on_the_wire() {
    let fixture = AcpWireFixture::start("negotiation", true).await;
    fixture.client.shutdown().await;
    let requests = fixture.requests();
    assert_eq!(
        requests[0]["params"]["clientCapabilities"]["_meta"]["parameterizedModelPicker"],
        true
    );
}

#[tokio::test]
async fn standard_initialize_omits_cursor_picker_metadata() {
    let fixture = AcpWireFixture::start("negotiation", false).await;
    fixture.client.shutdown().await;
    let requests = fixture.requests();
    assert!(
        requests[0]["params"]["clientCapabilities"]
            .get("_meta")
            .is_none()
    );
}

#[tokio::test]
async fn thinking_first_and_explicit_effort_win_without_companion_rewrites() {
    for scenario in ["thinking-first", "explicit-wins", "fallback"] {
        let fixture = AcpWireFixture::start(scenario, false).await;
        let session = fixture
            .client
            .create_session(fixture.root.path().into())
            .await
            .expect("session");
        let observed = fixture
            .client
            .set_setting(
                session.clone(),
                acp_client_runtime::ProviderSettingRequest {
                    kind: ProviderSettingKind::Effort,
                    value: "high".into(),
                    intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                    expected_model: None,
                },
            )
            .await;
        fixture.client.shutdown().await;
        let observed = observed.expect("independent advertised effort must apply");
        assert_eq!(observed.effort.as_deref(), Some("high"));
        let requests = fixture.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[2]["method"], "session/set_config_option");
        let config_id = if scenario == "fallback" {
            "budget"
        } else {
            "effort"
        };
        assert_eq!(
            requests[2]["params"],
            serde_json::json!({
            "sessionId":"fixture-session", "configId":config_id, "value":"high"})
        );
        let catalog = fixture
            .client
            .settings_catalog(&session)
            .await
            .expect("catalog");
        for (config_id, value) in [
            ("model", "fixture-a"),
            ("thinking", "true"),
            ("context", "300k"),
            ("fast", "false"),
        ] {
            assert_eq!(
                catalog
                    .config_options
                    .iter()
                    .find(|option| option.id == config_id)
                    .expect("companion option")
                    .current_value,
                ProviderConfigValue::Select(value.into())
            );
        }
        assert_eq!(
            catalog
                .config_options
                .iter()
                .find(|option| option.id == "thinking")
                .expect("thinking option")
                .category,
            None
        );
    }
}

#[tokio::test]
async fn ambiguous_or_thinking_only_effort_rejects_before_rpc() {
    for scenario in [
        "ambiguous",
        "thinking-only",
        "duplicate-explicit",
        "duplicate-mixed",
    ] {
        let fixture = AcpWireFixture::start(scenario, false).await;
        let session = fixture
            .client
            .create_session(fixture.root.path().into())
            .await
            .expect("session");
        let result = fixture
            .client
            .set_setting(
                session,
                acp_client_runtime::ProviderSettingRequest {
                    kind: ProviderSettingKind::Effort,
                    value: "high".into(),
                    intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                    expected_model: None,
                },
            )
            .await;
        fixture.client.shutdown().await;
        assert!(
            matches!(result, Err(ExternalProviderRuntimeError::InvalidSetting {
            ref advertised, .. }) if advertised.is_empty()),
            "{result:?}"
        );
        assert!(
            result
                .expect_err("ambiguous selection")
                .to_string()
                .contains("unambiguous")
        );
        assert_eq!(
            fixture.requests().len(),
            2,
            "rejected setting must not reach provider"
        );
    }
}

#[tokio::test]
async fn create_refreshes_mode_dependent_model_and_model_dependent_effort_choices() {
    let fixture = AcpWireFixture::start("dynamic", false).await;
    let result = fixture
        .client
        .create_session_with_settings(
            fixture.root.path().into(),
            RequestedProviderSettings {
                mode: Some("ask".into()),
                model: Some("fixture-b".into()),
                effort: Some("high".into()),
            },
        )
        .await;
    fixture.client.shutdown().await;
    let created = result.expect("fresh catalogs admit model then effort");
    assert_eq!(
        created.effective_settings.model.as_deref(),
        Some("fixture-b")
    );
    assert_eq!(created.effective_settings.effort.as_deref(), Some("high"));
    let requests = fixture.requests();
    assert_eq!(
        requests
            .iter()
            .skip(2)
            .map(|request| request["params"]["configId"].as_str().expect("config ID"))
            .collect::<Vec<_>>(),
        ["mode", "model", "effort"]
    );
}

#[tokio::test]
async fn provider_adjusted_companion_values_are_reported_in_full_settings_event() {
    let fixture = AcpWireFixture::start("adjusted", false).await;
    let session = fixture
        .client
        .create_session(fixture.root.path().into())
        .await
        .expect("session");
    let result = fixture
        .client
        .set_setting(
            session,
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Effort,
                value: "high".into(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await;
    fixture.client.shutdown().await;
    assert_eq!(
        result.expect("provider reports High").effort.as_deref(),
        Some("high")
    );
    let events = fixture.sink.settings.lock().expect("events");
    let reported = events.last().expect("returned settings event");
    for (config_id, value) in [
        ("effort", "high"),
        ("context", "1m"),
        ("fast", "true"),
        ("thinking", "true"),
    ] {
        assert_eq!(
            reported
                .config
                .iter()
                .find(|config| config.id == config_id)
                .expect("reported companion")
                .value,
            ConfigValueState::Select(value.into())
        );
    }
}

#[tokio::test]
async fn load_and_live_update_refresh_effort_identity_and_invalid_values_do_not_dispatch() {
    let fixture = AcpWireFixture::start("load-update", false).await;
    fixture
        .client
        .load_session("fixture-session".into(), fixture.root.path().into())
        .await
        .expect("load session");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let changed = fixture.sink.changed.notified();
            if fixture
                .client
                .settings_catalog("fixture-session")
                .await
                .expect("loaded catalog")
                .config_options
                .iter()
                .any(|option| option.id == "budget")
            {
                break;
            }
            changed.await;
        }
    })
    .await
    .expect("live complete catalog update");
    let rejected = fixture
        .client
        .set_setting(
            "fixture-session".into(),
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Effort,
                value: "not-offered".into(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await;
    assert!(matches!(
        rejected,
        Err(ExternalProviderRuntimeError::InvalidSetting { .. })
    ));
    let applied = fixture
        .client
        .set_setting(
            "fixture-session".into(),
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Effort,
                value: "high".into(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await;
    fixture.client.shutdown().await;
    assert_eq!(
        applied
            .expect("updated fallback effort applies")
            .effort
            .as_deref(),
        Some("high")
    );
    let requests = fixture.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["method"], "session/load");
    assert_eq!(requests[2]["params"]["configId"], "budget");
    assert_eq!(requests[2]["params"]["value"], "high");
}
