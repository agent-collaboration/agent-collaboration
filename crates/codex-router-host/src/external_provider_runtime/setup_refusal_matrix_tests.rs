//! Reachable initial-setting refusals follow mode/model/effort order on both login modes.
use super::{
    ExternalProviderRuntime, acp_scripted_fixture::AcpFixtureScript,
    setup_registrar_fixture::ProviderRegistrarFixture,
};
use acp_client_runtime::{
    ExternalProviderCreatedSession, ExternalProviderRuntimeError, ProviderSettingKind,
    RequestedProviderSettings,
};
use codex_router_fleet_state::fleet_session_registration::{
    AbandonedSessionCreation, FleetSessionBindingLifecycle, FleetSessionRegistrationRecord,
};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use serde_json::{Value, json};
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Copy, Debug)]
enum RefusalCase {
    InvalidFirst,
    ProviderRefused,
}
struct SettingRefusalObservation {
    created: Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError>,
    rows: Vec<FleetSessionRegistrationRecord>,
    registration_count: usize,
    calls: Vec<Value>,
    diagnostic: String,
}
fn advertised_setting_options(mode: &str, model: &str, effort: &str) -> Value {
    json!([
        {"id":"mode","name":"Mode","category":"mode","type":"select","currentValue":mode,"options":[{"value":"auto","name":"Auto"},{"value":"ask","name":"Ask"}]},
        {"id":"model","name":"Model","category":"model","type":"select","currentValue":model,"options":[{"value":"a","name":"A"},{"value":"b","name":"B"}]},
        {"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":effort,"options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}
    ])
}
async fn observe_setting_refusal(
    kind: ProviderSettingKind,
    case: RefusalCase,
    mode: SessionRegistrationMode,
) -> Result<SettingRefusalObservation, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir_in("/tmp")?;
    let finish_path = root.path().join("finished.sock");
    let finish = tokio::net::UnixListener::bind(&finish_path)?;
    let calls_path = root.path().join("calls.jsonl");
    let diagnostic_path = root.path().join("diagnostic.txt");
    let mut settings = RequestedProviderSettings::default();
    match (kind, case) {
        (ProviderSettingKind::Mode, RefusalCase::InvalidFirst) => {
            settings.mode = Some("not-offered".to_owned())
        }
        (ProviderSettingKind::Model, RefusalCase::InvalidFirst) => {
            settings.model = Some("not-offered".to_owned())
        }
        (ProviderSettingKind::Effort, RefusalCase::InvalidFirst) => {
            settings.effort = Some("not-offered".to_owned())
        }
        (ProviderSettingKind::Mode, RefusalCase::ProviderRefused) => {
            settings.mode = Some("ask".to_owned())
        }
        (ProviderSettingKind::Model, RefusalCase::ProviderRefused) => {
            settings.mode = Some("ask".to_owned());
            settings.model = Some("b".to_owned());
        }
        (ProviderSettingKind::Effort, RefusalCase::ProviderRefused) => {
            settings.mode = Some("ask".to_owned());
            settings.model = Some("b".to_owned());
            settings.effort = Some("high".to_owned());
        }
    }
    let registrar_fixture = ProviderRegistrarFixture::new(mode)?;
    let setup = registrar_fixture
        .create_setup(root.path(), &settings)
        .await?;
    let servers = match mode {
        SessionRegistrationMode::AccessEnrollment => json!([]),
        SessionRegistrationMode::MachineTokens => {
            json!([{"type":"http","name":"router-collaboration","url":"http://127.0.0.1:43210/mcp/session/35353535353535353535353535353535"}])
        }
    };
    let mut script = AcpFixtureScript::new()
        .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
        .respond("initialize", json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":true},"sessionCapabilities":{"close":{}}}}))
        .expect_request("create", "session/new", json!({"mcpServers":servers}))
        .respond("create", json!({"sessionId":"actual-provider-conversation","configOptions":advertised_setting_options("auto","a","low")}));
    if matches!(case, RefusalCase::ProviderRefused) {
        if kind != ProviderSettingKind::Mode {
            script = script.expect_request("mode", "session/set_config_option", json!({"sessionId":"actual-provider-conversation","configId":"mode","value":"ask"}))
                .respond("mode", json!({"configOptions":advertised_setting_options("ask","a","low")}));
        }
        if kind == ProviderSettingKind::Effort {
            script = script.expect_request("model", "session/set_config_option", json!({"sessionId":"actual-provider-conversation","configId":"model","value":"b"}))
                .respond("model", json!({"configOptions":advertised_setting_options("ask","b","low")}));
        }
        let (field, value) = match kind {
            ProviderSettingKind::Mode => ("mode", "ask"),
            ProviderSettingKind::Model => ("model", "b"),
            ProviderSettingKind::Effort => ("effort", "high"),
        };
        script = script
            .expect_request(
                "refused",
                "session/set_config_option",
                json!({"sessionId":"actual-provider-conversation","configId":field,"value":value}),
            )
            .respond_error("refused", -32602);
    } else if mode == SessionRegistrationMode::AccessEnrollment {
        script = script
            .expect_request(
                "close",
                "session/close",
                json!({"sessionId":"actual-provider-conversation"}),
            )
            .respond("close", json!({}));
    }
    let fixture = script
        .wait_for_socket_signal(&finish_path)
        .record_requests(&calls_path)
        .record_diagnostics(diagnostic_path.clone())
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture).await?;
    let created = runtime
        .create_session_with_settings(root.path().into(), settings, setup)
        .await;
    let retired = runtime.retirement();
    tokio::select! {
        _ = retired.cancelled() => {},
        accepted = finish.accept() => {
            let (mut acknowledgement, _) = accepted?;
            let _sent = acknowledgement.write_all(b"S").await;
            let mut completed = Vec::new();
            acknowledgement.read_to_end(&mut completed).await?;
        }
    }
    runtime.shutdown().await;
    let diagnostic = if diagnostic_path.exists() {
        std::fs::read_to_string(diagnostic_path)?
    } else {
        String::new()
    };
    let calls = std::fs::read_to_string(calls_path)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    let rows = registrar_fixture
        .repository
        .records
        .lock()
        .map_err(|_| io::Error::other("fixture store unavailable"))?
        .clone();
    let registration_count = registrar_fixture
        .authority
        .registrations
        .lock()
        .map_err(|_| io::Error::other("fixture authority unavailable"))?
        .len();
    Ok(SettingRefusalObservation {
        created,
        rows,
        registration_count,
        calls,
        diagnostic,
    })
}

#[tokio::test]
async fn provider_setup_refusals_follow_fixed_order_and_never_close_in_the_new_mode() {
    for mode in [
        SessionRegistrationMode::AccessEnrollment,
        SessionRegistrationMode::MachineTokens,
    ] {
        for kind in [
            ProviderSettingKind::Mode,
            ProviderSettingKind::Model,
            ProviderSettingKind::Effort,
        ] {
            for case in [RefusalCase::InvalidFirst, RefusalCase::ProviderRefused] {
                let observed = observe_setting_refusal(kind, case, mode).await.unwrap();
                assert!(
                    observed.diagnostic.is_empty(),
                    "{mode:?}/{kind:?}/{case:?}: {}",
                    observed.diagnostic
                );
                assert!(observed.created.is_err());
                assert_eq!(observed.registration_count, 0);
                let methods = observed
                    .calls
                    .iter()
                    .filter_map(|call| call["method"].as_str())
                    .collect::<Vec<_>>();
                assert_eq!(
                    methods
                        .iter()
                        .filter(|method| **method == "session/new")
                        .count(),
                    1
                );
                assert!(!methods.contains(&"session/prompt"));
                let closed = methods
                    .iter()
                    .filter(|method| **method == "session/close")
                    .count();
                let expected_settings = match case {
                    RefusalCase::InvalidFirst => 0,
                    RefusalCase::ProviderRefused => match kind {
                        ProviderSettingKind::Mode => 1,
                        ProviderSettingKind::Model => 2,
                        ProviderSettingKind::Effort => 3,
                    },
                };
                assert_eq!(
                    methods
                        .iter()
                        .filter(|method| **method == "session/set_config_option")
                        .count(),
                    expected_settings
                );
                if mode == SessionRegistrationMode::AccessEnrollment {
                    assert!(observed.rows.is_empty());
                    assert_eq!(
                        closed,
                        usize::from(matches!(case, RefusalCase::InvalidFirst))
                    );
                } else {
                    assert_eq!(closed, 0);
                    assert_eq!(observed.rows.len(), 1);
                    let FleetSessionBindingLifecycle::Abandoned {
                        creation: AbandonedSessionCreation::KnownCreated { session, cause },
                    } = observed.rows[0].lifecycle()
                    else {
                        panic!(
                            "new-mode refusal must retain the actual ID in abandoned state: {mode:?}/{kind:?}/{case:?}"
                        );
                    };
                    assert_eq!(session.session_id.as_str(), "actual-provider-conversation");
                    let setting = match kind {
                        ProviderSettingKind::Mode => "mode",
                        ProviderSettingKind::Model => "model",
                        ProviderSettingKind::Effort => "effort",
                    };
                    assert_eq!(
                        serde_json::to_value(cause).unwrap(),
                        json!({"kind":"initialSettingRefused","setting":setting})
                    );
                }
            }
        }
    }
}
