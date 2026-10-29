use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{OperationId, WakeSendRequest, WakeShowRequest};
use serde_json::json;
use std::{os::unix::fs::DirBuilderExt, time::Duration};

#[tokio::test]
async fn host_records_firing_without_native_backend_acceptance()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from("/tmp").join(format!(
        "host-wake-timer-{}",
        OperationId::generate().as_str()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
        directory: root.clone(),
        codex_home: root.clone(),
        backend_socket: root.join("unavailable-native.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: None,
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let client = CollaborationClient::connect(&root, "wake-timer-test", "1").await?;
    // Omitted expiry ends a day after due; a fixed historical date is already expired.
    let request: WakeSendRequest = serde_json::from_value(
        json!({"operationId":OperationId::generate(),"message":{"target":{"endpoint":{"serviceId":runtime.service_id(),"endpointId":"codex-local"},"sessionId":"fixture-only-new-thread"},"content":{"kind":"humanUser","text":"Check"},"delivery":"auto","generationGuard":null},"timing":{"kind":"at","at":chrono::Utc::now()},"requestedBy":{"kind":"owner"},"replace":false,"expiry":{"kind":"none"}}),
    )?;
    let created = client.send_wakeup(request).await?;
    let id = created.definition.wakeup_id;
    let fired = tokio::time::timeout(Duration::from_secs(3), async {
        client
            .clone()
            .subscribe_wakeup(WakeShowRequest {
                wakeup_id: id.clone(),
            })
            .await?
            .wait_until_first_fire()
            .await
    })
    .await??;
    if fired.wakeup_id != id {
        return Err("first-fire receipt named a different wake".into());
    }
    let observed = client
        .read_wakeup(WakeShowRequest { wakeup_id: id })
        .await?;
    if observed.pending_delivery_id.is_none() {
        return Err("firing lost durable delivery identity".into());
    }
    runtime.shutdown().await?;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err("unexpected owned fixture entry".into());
        }
        std::fs::remove_file(entry.path())?;
    }
    std::fs::remove_dir(root)?;
    Ok(())
}
