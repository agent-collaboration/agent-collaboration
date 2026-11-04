use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::{CollaborationClient, JournalStatus};
use collaboration_protocol::EndpointAvailability;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn journal_open_failure_does_not_disable_the_api_and_native_publication() {
    // Arrange: an owner-private fixture path deliberately cannot be opened as SQLite.
    let directory = tempfile::Builder::new()
        .prefix("journal-isolation-")
        .tempdir()
        .unwrap_or_else(|error| panic!("directory: {error}"));
    let root = directory.path().to_path_buf();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("directory permissions: {error}"));
    let blocked_database = root.join("session-registry.sqlite");
    std::fs::create_dir(&blocked_database)
        .unwrap_or_else(|error| panic!("blocked database: {error}"));
    // Act: callers can still discover the service and its explicit storage-unavailable state.
    let mut runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
        directory: root.clone(),
        codex_home: root.clone(),
        backend_socket: root.join("backend.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: None,
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await
    .unwrap_or_else(|error| panic!("communication startup: {error}"));
    let client = CollaborationClient::connect(&root, "journal-failure-proof", "1")
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    let status = client
        .journal_status()
        .await
        .unwrap_or_else(|error| panic!("status: {error}"));
    runtime
        .backend_ready(
            "2026-09-05T12:00:00Z"
                .to_owned()
                .try_into()
                .unwrap_or_else(|error| panic!("time: {error}")),
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("fixture readiness: {error}"));
    let endpoints = client
        .list_endpoints()
        .await
        .unwrap_or_else(|error| panic!("endpoints: {error}"));
    let listener_present = root.join("codex-native.sock").exists();
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
    // Assert: this proves listener/publication independence, not a running Codex backend.
    assert!(matches!(status, JournalStatus::Unavailable));
    assert!(listener_present);
    assert!(matches!(
        endpoints
            .endpoints
            .first()
            .map(|endpoint| &endpoint.availability),
        Some(EndpointAvailability::Available { .. })
    ));
}
