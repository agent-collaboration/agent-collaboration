//! Actual Host composition reaches the fixed source-local native command.
//! Its external executable supplies fixture observations; no vendor or model runs.
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderStartup,
};
use collaboration_protocol::{
    NativeClaudeBackgroundState, NativeSessionScope, NativeSessionSource, NativeSessionView,
    ProviderKind, ProviderSessionListParams, ProviderSessionSummary,
};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

const SESSION: &str = "00000000-0000-4000-8000-000000000042";

#[tokio::test]
async fn host_composes_job_discovery_when_canonical_claude_has_no_acp_channel() {
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("claude");
    node_test_fixture::write_node_fixture(&binary, "native-cli-catalogue-agent.ts", &json!({"kind":"file","output":{"kind":"environment","name":"NATIVE_CATALOGUE_OUTPUT"},"readLog":{"kind":"environment","name":"NATIVE_CATALOGUE_READS"}})).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_host_native_catalogue_probe",
                "--ignored",
                "--nocapture",
            ])
            .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
            .env("NATIVE_CATALOGUE_ROOT", root.path())
            .env("HOME", root.path())
            .env("NATIVE_CATALOGUE_READS", root.path().join("reads.txt"))
            .env(
                "NATIVE_CATALOGUE_OUTPUT",
                root.path().join("observations.json"),
            )
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("reads.txt"))
            .unwrap()
            .lines()
            .count(),
        3,
        "exactly the requested three native reads, no retry/prompt/lifecycle command"
    );
}

#[tokio::test]
#[ignore = "Isolated Host fixture child supplies a private process PATH"]
async fn isolated_host_native_catalogue_probe() {
    let root = PathBuf::from(std::env::var_os("NATIVE_CATALOGUE_ROOT").unwrap());
    let observed = PathBuf::from(std::env::var_os("NATIVE_CATALOGUE_OUTPUT").unwrap());
    let peers = root.join("peers");
    std::fs::create_dir(&peers).unwrap();
    let api_root = root.join("host");
    std::fs::create_dir(&api_root).unwrap();
    std::fs::set_permissions(&api_root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let job = |state| json!([{"kind":"background","id":"separate-host-job-handle","sessionId":SESSION,"cwd":"/source/owned-project","name":"Before prompt","state":state,"startedAt":1790162100000_i64}]);
    std::fs::write(&observed, serde_json::to_vec(&job("blocked")).unwrap()).unwrap();
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: None,
            directory: api_root.clone(),
            codex_home: root.join("codex-home"),
            backend_socket: root.join("unused-native.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: Some(peers),
            remote_control_server_name: None,
            owner_human_id: None,
        },
        vec![ExternalProviderStartup::Unavailable {
            provider: ProviderKind::ClaudeCode,
            reason: "ACP intentionally unavailable"
                .to_owned()
                .try_into()
                .unwrap(),
            fix: "Native discovery remains independent"
                .to_owned()
                .try_into()
                .unwrap(),
        }],
    )
    .await
    .unwrap();
    let client =
        collaboration_client::CollaborationClient::connect(&api_root, "host-native-catalogue", "1")
            .await
            .unwrap();
    let inventory = client.list_endpoints().await.unwrap();
    let endpoint = inventory
        .endpoints
        .iter()
        .find(|entry| String::from(entry.endpoint.endpoint_id.clone()) == "claude-local")
        .unwrap();
    assert!(endpoint.channels.is_empty());
    let request = ProviderSessionListParams {
        endpoint: endpoint.endpoint.clone(),
        view: NativeSessionView::Loaded,
        scope: NativeSessionScope::Any,
        source: NativeSessionSource::Interactive,
        query: Some(SESSION.to_owned()),
        page_size: 100,
        cursor: None,
    };
    let loaded = client
        .list_provider_sessions(request.clone())
        .await
        .unwrap();
    assert_eq!(loaded.sessions.len(), 1);
    assert!(
        matches!(loaded.sessions.first().unwrap(),ProviderSessionSummary::ClaudeCodeBackground {target,job_handle,state:NativeClaudeBackgroundState::Blocked,peer:None,..} if String::from(target.session_id.clone())==SESSION&&String::from(job_handle.clone())=="separate-host-job-handle")
    );
    std::fs::write(observed, serde_json::to_vec(&job("stopped")).unwrap()).unwrap();
    let stored = client
        .list_provider_sessions(ProviderSessionListParams {
            view: NativeSessionView::Stored,
            ..request.clone()
        })
        .await
        .unwrap();
    assert!(matches!(
        stored.sessions.first().unwrap(),
        ProviderSessionSummary::ClaudeCodeBackground {
            state: NativeClaudeBackgroundState::Stopped,
            ..
        }
    ));
    let no_longer_loaded = client.list_provider_sessions(request).await.unwrap();
    assert!(no_longer_loaded.sessions.is_empty());
    assert_eq!(
        no_longer_loaded.native_coverage.unwrap().background,
        collaboration_protocol::NativeClaudeInventoryPart::Complete
    );
    drop(client);
    runtime.shutdown().await.unwrap();
}
