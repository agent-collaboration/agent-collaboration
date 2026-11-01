//! Actual scripted ACP transport refuses a setting request until the created-ID save is acknowledged.
use super::{ExternalProviderRuntime, acp_scripted_fixture::AcpFixtureScript};
use acp_client_runtime::{
    ProviderSessionCreateSetup, ProviderSessionSetupFuture, ProviderSessionSetupPort,
    ProviderSessionSetupReadiness, ProviderSessionSetupUnavailable, ProviderSettingKind,
    RequestedProviderSettings,
};
use codex_router_fleet_state::session_registration_repository::FleetSessionStorageFailure;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct RecordedSetup {
    saved: tokio::net::UnixListener,
    ids: Mutex<Vec<String>>,
    events: Mutex<Vec<&'static str>>,
}
impl ProviderSessionSetupPort for RecordedSetup {
    fn created<'a>(&'a self, provider_session_id: &'a str) -> ProviderSessionSetupFuture<'a, ()> {
        Box::pin(async move {
            let (mut acknowledgement, _) = self
                .saved
                .accept()
                .await
                .map_err(|_| ProviderSessionSetupUnavailable)?;
            self.ids
                .lock()
                .map_err(|_| ProviderSessionSetupUnavailable)?
                .push(provider_session_id.to_owned());
            self.events
                .lock()
                .map_err(|_| ProviderSessionSetupUnavailable)?
                .push("createdSaved");
            acknowledgement
                .write_all(b"S")
                .await
                .map_err(|_| ProviderSessionSetupUnavailable)?;
            // Peer closure acknowledges that its no-setting observer has been removed.
            let mut acknowledged = Vec::new();
            acknowledgement
                .read_to_end(&mut acknowledged)
                .await
                .map_err(|_| ProviderSessionSetupUnavailable)?;
            Ok(())
        })
    }
    fn setup_passed<'a>(
        &'a self,
        _provider_session_id: &'a str,
    ) -> ProviderSessionSetupFuture<'a, ProviderSessionSetupReadiness> {
        Box::pin(async move {
            self.events
                .lock()
                .map_err(|_| ProviderSessionSetupUnavailable)?
                .push("setupPassed");
            Ok(ProviderSessionSetupReadiness::Ready)
        })
    }
    fn setup_refused<'a>(
        &'a self,
        _provider_session_id: &'a str,
        _setting: ProviderSettingKind,
    ) -> ProviderSessionSetupFuture<'a, ()> {
        Box::pin(async move {
            self.events
                .lock()
                .map_err(|_| ProviderSessionSetupUnavailable)?
                .push("setupRefused");
            Ok(())
        })
    }
}

#[tokio::test]
async fn provider_setup_retains_the_returned_id_before_any_initial_setting() {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let saved_socket = directory.path().join("saved.sock");
    let setup = Arc::new(RecordedSetup {
        saved: tokio::net::UnixListener::bind(&saved_socket).unwrap(),
        ids: Mutex::new(Vec::new()),
        events: Mutex::new(Vec::new()),
    });
    let settings_marker = directory.path().join("setting-observed");
    let fixture = AcpFixtureScript::new()
        .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
        .respond("initialize", json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":true},"sessionCapabilities":{"close":{}}}}))
        .expect_request("create", "session/new", json!({"mcpServers":[{"type":"http","name":"router-collaboration","url":"http://127.0.0.1:43210/mcp/session/11111111111111111111111111111111"}]}))
        .respond("create", json!({"sessionId":"actual-provider-conversation","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"}]}}))
        .wait_for_socket_signal(&saved_socket)
        .expect_request("mode", "session/set_mode", json!({"sessionId":"actual-provider-conversation","modeId":"ask"}))
        .write_marker(&settings_marker)
        .respond("mode", json!({}))
        .record_diagnostics(directory.path().join("wire-diagnostic.txt"))
        .launch();
    let runtime = ExternalProviderRuntime::initialize(fixture).await.unwrap();

    let created = runtime
        .create_session_with_settings(
            directory.path().into(),
            RequestedProviderSettings {
                mode: Some("ask".to_owned()),
                ..Default::default()
            },
            ProviderSessionCreateSetup::Managed {
                server_name: "router-collaboration".to_owned(),
                server_url: "http://127.0.0.1:43210/mcp/session/11111111111111111111111111111111"
                    .to_owned(),
                port: setup.clone(),
            },
        )
        .await;

    assert_eq!(
        *setup.events.lock().unwrap(),
        ["createdSaved", "setupPassed"],
        "{}",
        std::fs::read_to_string(directory.path().join("wire-diagnostic.txt")).unwrap_or_default()
    );
    assert_eq!(*setup.ids.lock().unwrap(), ["actual-provider-conversation"]);
    assert_eq!(
        created.unwrap().provider_session_id,
        "actual-provider-conversation"
    );
    assert!(settings_marker.exists());
    runtime.shutdown().await;
}

#[tokio::test]
async fn provider_setup_failed_pending_save_applies_no_setting_through_the_real_registrar() {
    use collaboration_service::fleet_session_registrar::{
        SessionRegistrationMode, test_support::PendingSetupWriteControl,
    };
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let registrar_fixture = super::setup_registrar_fixture::ProviderRegistrarFixture::new(
        SessionRegistrationMode::MachineTokens,
    )
    .unwrap();
    let settings = RequestedProviderSettings {
        mode: Some("ask".to_owned()),
        ..Default::default()
    };
    let managed = registrar_fixture
        .create_setup(directory.path(), &settings)
        .await
        .unwrap();
    let (entered, pending_save) = tokio::sync::oneshot::channel();
    let (decide, outcome) = tokio::sync::oneshot::channel();
    *registrar_fixture.repository.pending_setup.lock().unwrap() =
        Some(PendingSetupWriteControl { entered, outcome });
    let marker = directory.path().join("setting-observed");
    let fixture = AcpFixtureScript::new()
        .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
        .respond("initialize", json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":true},"sessionCapabilities":{"close":{}}}}))
        .expect_request("create", "session/new", json!({"mcpServers":[{"type":"http","name":"router-collaboration","url":"http://127.0.0.1:43210/mcp/session/35353535353535353535353535353535"}]}))
        .respond("create", json!({"sessionId":"actual-provider-conversation","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"}]}}))
        .expect_request("mode", "session/set_mode", json!({"sessionId":"actual-provider-conversation","modeId":"ask"}))
        .write_marker(&marker)
        .respond("mode", json!({}))
        .launch();
    let runtime = Arc::new(ExternalProviderRuntime::initialize(fixture).await.unwrap());
    let running = Arc::clone(&runtime);
    let cwd = directory.path().to_owned();
    let mut creating = tokio::spawn(async move {
        running
            .create_session_with_settings(cwd, settings, managed)
            .await
    });

    tokio::select! {
        entered = pending_save => entered.unwrap(),
        result = &mut creating => panic!("create settled without awaiting pending-setup persistence: {result:?}"),
    }
    assert!(
        !marker.exists(),
        "a setting arrived while the created-ID save was held"
    );
    decide
        .send(Err(FleetSessionStorageFailure::Unavailable))
        .unwrap();
    assert!(
        matches!(creating.await.unwrap(), Err(acp_client_runtime::ExternalProviderRuntimeError::SessionSetupFailed {
        provider_session_id, failure: acp_client_runtime::ProviderSessionSetupFailure::CreatedSaveUnconfirmed,
    }) if provider_session_id == "actual-provider-conversation")
    );

    assert!(
        !marker.exists(),
        "a setting arrived after the created-ID save failed"
    );
    assert!(
        registrar_fixture
            .authority
            .registrations
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        registrar_fixture
            .registrar
            .resolve_binding_capability("35353535353535353535353535353535")
            .await,
        Err(collaboration_service::fleet_session_registrar::RegistrarFailure::InactiveBinding)
    ));
    {
        let records = registrar_fixture.repository.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].lifecycle().session().is_none());
    }
    runtime.shutdown().await;
}

#[tokio::test]
async fn provider_setup_good_settings_register_the_actual_conversation_only_in_the_new_mode() {
    use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
    for mode in [
        SessionRegistrationMode::AccessEnrollment,
        SessionRegistrationMode::MachineTokens,
    ] {
        let directory = tempfile::tempdir_in("/tmp").unwrap();
        let registrar_fixture =
            super::setup_registrar_fixture::ProviderRegistrarFixture::new(mode).unwrap();
        let settings = RequestedProviderSettings {
            mode: Some("ask".to_owned()),
            ..Default::default()
        };
        let setup = registrar_fixture
            .create_setup(directory.path(), &settings)
            .await
            .unwrap();
        let servers = match mode {
            SessionRegistrationMode::AccessEnrollment => json!([]),
            SessionRegistrationMode::MachineTokens => {
                json!([{"type":"http","name":"router-collaboration","url":"http://127.0.0.1:43210/mcp/session/35353535353535353535353535353535"}])
            }
        };
        let fixture = AcpFixtureScript::new()
            .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
            .respond("initialize", json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":true}}}))
            .expect_request("create", "session/new", json!({"mcpServers":servers}))
            .respond("create", json!({"sessionId":"actual-provider-conversation","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"}]}}))
            .expect_request("mode", "session/set_mode", json!({"sessionId":"actual-provider-conversation","modeId":"ask"}))
            .respond("mode", json!({}))
            .launch();
        let runtime = ExternalProviderRuntime::initialize(fixture).await.unwrap();

        let created = runtime
            .create_session_with_settings(directory.path().into(), settings, setup)
            .await
            .unwrap();

        assert_eq!(created.provider_session_id, "actual-provider-conversation");
        if mode == SessionRegistrationMode::AccessEnrollment {
            assert!(
                registrar_fixture
                    .repository
                    .records
                    .lock()
                    .unwrap()
                    .is_empty()
            );
            assert!(
                registrar_fixture
                    .authority
                    .registrations
                    .lock()
                    .unwrap()
                    .is_empty()
            );
        } else {
            let registrations = registrar_fixture
                .authority
                .registrations
                .lock()
                .unwrap()
                .clone();
            assert_eq!(registrations.len(), 1);
            assert_eq!(
                registrations[0].session.session_id.as_str(),
                "actual-provider-conversation"
            );
            assert_eq!(
                registrations[0].isolation.kind,
                fleet_identity_protocol::IsolationKind::Host
            );
            assert_eq!(
                registrations[0].machine_environment,
                fleet_identity_protocol::MachineEnvironment::CloudVm
            );
            assert_eq!(
                registrations[0].started_by,
                fleet_identity_protocol::StartedBy::RouterOperator {}
            );
            assert!(
                registrar_fixture
                    .registrar
                    .resolve_binding_capability("35353535353535353535353535353535")
                    .await
                    .is_ok()
            );
        }
        runtime.shutdown().await;
    }
}
