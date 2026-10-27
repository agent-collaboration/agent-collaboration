//! Consumer proofs over the actual Host composition, with weak test-only observers.
use super::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::CollaborationClient;
use collaboration_protocol::SessionRef;
use std::{os::unix::fs::PermissionsExt as _, sync::Arc, time::Duration};

const BOUND: Duration = Duration::from_secs(20);

#[path = "startup_contract_tests/broker_context_tests.rs"]
mod broker_context_tests;
#[path = "startup_contract_tests/follow_attachment_tests.rs"]
mod follow_attachment_tests;
#[path = "../../tests/support/registered_route_project.rs"]
mod registered_route_project;
#[path = "../../tests/support/session_follow_fixture.rs"]
mod route_fixture;
#[path = "startup_contract_tests/wake_capacity_tests.rs"]
mod wake_capacity_tests;

struct StartupFixture {
    directory: tempfile::TempDir,
    runtime: CollaborationRuntime,
    _central: Option<board_central_test_support::TestCentral>,
}

async fn start_host(enrolled: bool) -> StartupFixture {
    let directory = tempfile::Builder::new()
        .prefix("host-contract-")
        .tempdir_in("/tmp")
        .expect("private Host root");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private permissions");
    std::fs::create_dir(directory.path().join("peers")).expect("peer registry");
    std::fs::set_permissions(
        directory.path().join("peers"),
        std::fs::Permissions::from_mode(0o700),
    )
    .expect("private peer registry");
    let service = collaboration_runtime::load_service_identity(directory.path()).expect("service");
    let machine = collaboration_contracts::MachineIdentity::new(service.clone(), None)
        .expect("machine identity");
    let central = if enrolled {
        Some(
            board_central_test_support::TestCentral::start_with_peer(
                &directory.path().join("central"),
                &message_board::ActorOrigin {
                    installation: String::from(service).try_into().expect("installation"),
                    machine: machine
                        .machine_label()
                        .as_str()
                        .to_owned()
                        .try_into()
                        .expect("label"),
                },
                collaboration_runtime::load_installation_peer(directory.path()).expect("peer"),
            )
            .await
            .expect("enrolled central"),
        )
    } else {
        None
    };
    let runtime = Box::pin(CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: central
            .as_ref()
            .map(|central| central.session_inputs().expect("session")),
        owner_human_id: None,
        directory: directory.path().to_owned(),
        codex_home: directory.path().to_owned(),
        backend_socket: directory.path().join("missing-native.sock"),
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        native_schema: None,
        peer_registry_directory: Some(directory.path().join("peers")),
        remote_control_server_name: None,
    }))
    .await
    .expect("actual Host construction");
    StartupFixture {
        directory,
        runtime,
        _central: central,
    }
}

fn target(runtime: &CollaborationRuntime, session: &str) -> SessionRef {
    serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":runtime.service_id(),"endpointId":"claude-local"},
        "sessionId":session
    }))
    .expect("local target")
}

async fn client(fixture: &StartupFixture) -> CollaborationClient {
    CollaborationClient::connect(fixture.directory.path(), "host-contract-tests", "1")
        .await
        .expect("actual Host API")
}
