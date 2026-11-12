//! Consumer proofs over the actual Host composition, with weak test-only observers.
use super::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_protocol::SessionRef;
use std::{os::unix::fs::PermissionsExt as _, time::Duration};

const BOUND: Duration = Duration::from_secs(20);

#[path = "startup_contract_tests/broker_context_tests.rs"]
mod broker_context_tests;

struct StartupFixture {
    directory: tempfile::TempDir,
    runtime: CollaborationRuntime,
}

async fn start_host() -> StartupFixture {
    let directory = tempfile::Builder::new()
        .prefix("host-contract-")
        .tempdir_in("/tmp")
        .expect("private Host root");
    start_host_in(directory).await
}

async fn start_host_in(directory: tempfile::TempDir) -> StartupFixture {
    start_host_with_providers(directory, Vec::new()).await
}

/// Starts the Host with these provider entries; an unavailable one launches nothing.
async fn start_host_with_providers(
    directory: tempfile::TempDir,
    provider_launches: Vec<crate::ExternalProviderStartup>,
) -> StartupFixture {
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private permissions");
    std::fs::create_dir(directory.path().join("peers")).expect("peer registry");
    std::fs::set_permissions(
        directory.path().join("peers"),
        std::fs::Permissions::from_mode(0o700),
    )
    .expect("private peer registry");
    let runtime = start_runtime_with_providers(directory.path(), provider_launches).await;
    StartupFixture { directory, runtime }
}

fn target(runtime: &CollaborationRuntime, session: &str) -> SessionRef {
    serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":runtime.service_id(),"endpointId":"claude-local"},
        "sessionId":session
    }))
    .expect("local target")
}

/// Starts the actual Host runtime in `directory` with these provider entries.
async fn start_runtime_with_providers(
    directory: &std::path::Path,
    provider_launches: Vec<crate::ExternalProviderStartup>,
) -> CollaborationRuntime {
    Box::pin(CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            owner_human_id: None,
            directory: directory.to_owned(),
            codex_home: directory.to_owned(),
            backend_socket: directory.join("missing-native.sock"),
            mcp_bind: ([127, 0, 0, 1], 0).into(),
            native_schema: None,
            peer_registry_directory: Some(directory.join("peers")),
            remote_control_server_name: None,
        },
        provider_launches,
    ))
    .await
    .expect("actual Host construction")
}
