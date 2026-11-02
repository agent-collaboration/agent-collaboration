use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    EndpointRef, InstructionCreateParams, InstructionText, OperationId, ScheduleShowRequest,
    SessionRef,
};
use std::os::unix::fs::DirBuilderExt;

#[tokio::test]
async fn host_supplies_retained_local_automation_storage_without_native_backend()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a private, short test socket directory; no native process is launched.
    let root = std::path::PathBuf::from(format!(
        "/tmp/host-automation-{}",
        OperationId::generate().as_str()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    // Retained Local definitions predate the public create cutover to central.
    let mut store =
        automation_storage::AutomationStore::open(&root.join("automation.sqlite")).await?;
    let instruction = store
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("Check the repository".to_owned())?,
            0,
        )
        .await?;
    let schedule = store
        .create_schedule(
            &automation_storage::ScheduleCreate::<SessionRef, EndpointRef> {
                operation_id: OperationId::generate(),
                definition: agent_automation::ScheduleDefinition {
                    instruction_id: instruction.instruction_id.clone(),
                    timing: agent_automation::TimingRule::Interval { seconds: 600 },
                    enabled: false,
                    destination: agent_automation::ExecutionDestination::Unprepared,
                    execution_timeout_seconds: None,
                    model: Some("gpt-6-luna".into()),
                    effort: Some("low".into()),
                },
                imported_continuity: agent_automation::ContinuityInput::None,
                now_ms: 0,
            },
        )
        .await?;
    store.close().await?;
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
    let operation = OperationId::generate();
    let proof: Result<(), Box<dyn std::error::Error>> = async {
    // Act: actual SDK reads the retained Local owner through the Host-composed store.
    let shown = client.read_schedule(ScheduleShowRequest {schedule_id:schedule.schedule_id.clone()}).await?;
    if !matches!(shown, collaboration_client::ScheduleReadResponse::Local {value}
        if value.definition.instruction_id == instruction.instruction_id && value.active_run_id.is_none() && value.waiting_run_id.is_none()) {
        return Err("Host did not serve the retained Local Schedule and its real occupancy".into());
    }
    let exported = client.export_schedule(ScheduleShowRequest {schedule_id:schedule.schedule_id}).await?;
    let package = agent_automation::decode_schedule_package::<SessionRef, EndpointRef>(
        &exported.package_utf8, collaboration_protocol::MAX_CONTROL_FRAME_BYTES,
    )?;
    // New public creation still requires central, even while Local storage is available.
    let refused = client
        .create_instruction(InstructionCreateParams {
            operation_id: operation.clone(),
            text: InstructionText::try_from("New central-only instructions".to_owned())?,
        })
        .await;
    if !matches!(refused, Err(collaboration_client::InstructionClientError::Catalog(
        collaboration_client::CentralCatalogFailure::CentralUnreachable {operation_id:Some(actual),..}
    )) if actual == operation) {
        return Err("new Instruction create fell back to Local storage or lost operation identity".into());
    }
    // Assert: separate automation database exists and contains the requested current text.
    if package.instruction.text.as_str() != "Check the repository"
        || !root.join("automation.sqlite").exists()
        || !root.join("session-registry.sqlite").exists()
    {
        return Err("Host did not compose separate automation persistence".into());
    }
    Ok(())
    }.await;
    runtime.shutdown().await?;
    let mut store =
        automation_storage::AutomationStore::open(&root.join("automation.sqlite")).await?;
    let no_local_create = matches!(
        store.read_operation(&operation).await,
        Err(automation_storage::StorageError::OperationNotFound)
    );
    store.close().await?;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err("unexpected runtime cleanup entry".into());
        }
        std::fs::remove_file(entry.path())?;
    }
    std::fs::remove_dir(root)?;
    proof?;
    if !no_local_create {
        return Err("unavailable central create admitted a Local operation".into());
    }
    Ok(())
}
