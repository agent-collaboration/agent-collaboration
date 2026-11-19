use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{BoardFailureKind, PageLimit, PageRequest, WorkspaceListRequest};
use std::os::unix::fs::DirBuilderExt;

#[path = "../support/native_project_fixture.rs"]
mod native_project_fixture;

#[tokio::test]
async fn board_open_failure_keeps_unrelated_control_methods_available()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(format!(
        "/tmp/host-board-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    std::fs::create_dir(root.join(message_board_storage::FleetStore::FILE_NAME))?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
        directory: root.clone(),
        codex_home: root.clone(),
        backend_socket: root.join("absent-native.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: Some(root.join("test-peer-registry")),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let client = CollaborationClient::connect(&root, "board-failure-isolation", "1").await?;

    let _endpoints = client.list_endpoints().await?;
    let board_result = client
        .workspace_list(WorkspaceListRequest {
            repository: None,
            page: PageRequest {
                limit: PageLimit::try_from(10)?,
                cursor: None,
            },
        })
        .await;
    runtime.shutdown().await?;
    if root.join("provider-operations.sqlite-wal").exists()
        || root.join("provider-operations.sqlite-shm").exists()
    {
        return Err("provider operation sidecars outlived runtime shutdown".into());
    }
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    std::fs::remove_dir(root)?;
    if !matches!(
        board_result,
        Err(BoardClientError::Rejected(error)) if error.kind == BoardFailureKind::BoardUnavailable
    ) {
        return Err(
            "unavailable board store did not return the typed boardUnavailable error".into(),
        );
    }
    Ok(())
}

#[tokio::test]
async fn corrupt_project_at_startup_keeps_runtime_and_endpoints_available()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    std::fs::set_permissions(root, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    let fleet_store = message_board_storage::FleetStore::open(
        &(root).join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await?;
    let fleet = collaboration_runtime::FleetClient::new(
        &fleet_store,
        std::sync::Arc::new(native_project_fixture::UnsettledRegistration),
        None,
    );
    drop(fleet);
    fleet_store.close().await;
    std::fs::create_dir(root.join("projects"))?;
    let broken = message_board::ProjectId::generate();
    let project_directory = root.join("projects").join(broken.as_str());
    std::fs::create_dir(&project_directory)?;
    std::fs::write(project_directory.join("e1.sqlite"), b"corrupt native file")?;
    let offline_endpoint = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: Some(native_project_fixture::offline_inputs(
            offline_endpoint.local_addr()?,
            collaboration_runtime::load_installation_peer(root)?,
        )?),
        directory: root.to_path_buf(),
        codex_home: root.to_path_buf(),
        backend_socket: root.join("absent-native.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: Some(root.join("test-peer-registry")),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let client = CollaborationClient::connect(root, "corrupt-project-isolation", "1").await?;
    let _endpoints = client.list_endpoints().await?;
    let _workspaces = client
        .workspace_list(WorkspaceListRequest {
            repository: None,
            page: PageRequest {
                limit: PageLimit::try_from(10)?,
                cursor: None,
            },
        })
        .await?;
    runtime.shutdown().await?;
    Ok(())
}
