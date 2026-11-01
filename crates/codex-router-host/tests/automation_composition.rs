use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{InstructionCreateParams, InstructionText, OperationId};
use collaboration_runtime::test_support::remove_store_directory_tolerating_sqlite_sidecars;
use std::os::unix::fs::DirBuilderExt;

#[tokio::test]
async fn host_supplies_automation_storage_without_native_backend()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a private, short test socket directory; no native process is launched.
    let root = std::path::PathBuf::from(format!(
        "/tmp/host-automation-{}",
        OperationId::generate().as_str()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    let legacy_approval_history = root.join("approval-history.json");
    std::fs::write(&legacy_approval_history, b"{malformed approval history")?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
        directory: root.clone(),
        codex_home: root.clone(),
        backend_socket: root.join("absent-native.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: None,
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let client = CollaborationClient::connect(&root, "automation-composition", "1").await?;
    // Act: Host-created service handles a real SDK request with no native endpoint.
    let document = client
        .create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(),
            text: InstructionText::try_from("Check the repository".to_owned())?,
        })
        .await?;
    // Assert: separate automation database exists and contains the requested current text.
    if document.text.as_str() != "Check the repository"
        || !root.join("automation.sqlite").exists()
        || !root.join("session-registry.sqlite").exists()
        || legacy_approval_history.exists()
    {
        return Err(
            "Host did not compose automation persistence or remove legacy approval history".into(),
        );
    }
    runtime
        .shutdown()
        .await
        .map_err(|error| format!("runtime shutdown: {error}"))?;
    remove_store_directory_tolerating_sqlite_sidecars(&root)
        .map_err(|error| format!("remove test root {}: {error}", root.display()))?;
    Ok(())
}
