//! Actual scripted ACP transport refuses a setting request until the created-ID save is acknowledged.
use super::{ExternalProviderRuntime, acp_scripted_fixture::AcpFixtureScript};
use acp_client_runtime::{
    ProviderSessionCreateSetup, ProviderSessionSetupFuture, ProviderSessionSetupPort,
    ProviderSessionSetupReadiness, ProviderSessionSetupUnavailable, ProviderSettingKind,
    RequestedProviderSettings,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

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
