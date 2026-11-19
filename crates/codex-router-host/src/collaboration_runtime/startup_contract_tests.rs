//! Consumer proofs over the actual Host composition, with weak test-only observers.
use super::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_protocol::SessionRef;
use std::{os::unix::fs::PermissionsExt as _, sync::Arc, time::Duration};

const BOUND: Duration = Duration::from_secs(20);

#[path = "startup_contract_tests/broker_context_tests.rs"]
mod broker_context_tests;
#[path = "startup_contract_tests/observation_recheck_tests.rs"]
mod observation_recheck_tests;
#[path = "startup_contract_tests/scheduler_completion_tests.rs"]
mod scheduler_completion_tests;
#[path = "startup_contract_tests/status_cancellation_test.rs"]
mod status_cancellation_test;

#[tokio::test]
async fn whole_host_shutdown_retains_the_native_failure_when_link_observation_fails() {
    for native in [
        fleet_sync_session::ClientError::Protocol,
        fleet_sync_session::ClientError::Revoked,
    ] {
        // Arrange: real runtime owners, with a failed session completion and unavailable diagnostic.
        let mut fixture = start_host().await;
        let expected = native.to_string();
        let expected_variant = std::mem::discriminant(&native);
        fixture.runtime.board_session = Some(
            super::board_composition::RunningMachineSession::failed_for_test(
                std::io::Error::other(native),
            ),
        );
        fixture.runtime.fleet_link = board_fleet_sync::FleetLinkObservation::Unavailable {
            message: "link persistence reader failed independently".to_owned(),
        };

        // Act: exercise the complete Host shutdown arm, not its task reduction helper.
        let error = fixture
            .runtime
            .shutdown()
            .await
            .expect_err("session failed");

        // Assert: the actual typed root cause survives both failure domains.
        let retained = error
            .get_ref()
            .and_then(|error| error.downcast_ref::<fleet_sync_session::ClientError>())
            .expect("native session cause was replaced by the link read failure");
        assert_eq!(std::mem::discriminant(retained), expected_variant);
        assert_eq!(retained.to_string(), expected);
    }
}

/// A local, unenrolled Host: no central, no machine session.
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
    let runtime = start_runtime_with_providers(directory.path(), None, provider_launches).await;
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
    board_session: Option<board_fleet_sync::BoardSessionInputs>,
    provider_launches: Vec<crate::ExternalProviderStartup>,
) -> CollaborationRuntime {
    Box::pin(CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session,
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
