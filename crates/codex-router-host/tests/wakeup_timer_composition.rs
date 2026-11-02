//! Pre-upgrade local timers retain their actual first-fire and delivery evidence before handover.
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{OperationId, WakeShowRequest};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, time::Duration};

#[tokio::test]
async fn host_records_legacy_local_firing_without_native_backend_acceptance()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::Builder::new()
        .prefix("host-legacy-wake-")
        .tempdir_in("/tmp")?;
    let root = directory.path();
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    std::fs::create_dir(root.join("peers"))?;
    std::fs::set_permissions(root.join("peers"), std::fs::Permissions::from_mode(0o700))?;
    let service = collaboration_runtime::load_service_identity(root)?;
    let mut store =
        automation_storage::AutomationStore::open(&root.join("automation.sqlite")).await?;
    let created = store.create_wakeup(&automation_storage::WakeCreate {
        operation_id:OperationId::generate(),requested_by:collaboration_protocol::WakeCreator::Owner {},replace:false,
        message:serde_json::from_value::<collaboration_protocol::SavedMessage>(json!({
            "target":{"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"fixture-only-existing-wake"},
            "content":{"kind":"humanUser","text":"Check"},"delivery":"auto","generationGuard":null
        }))?,
        timing:agent_automation::TimingRule::At { at:chrono::Utc::now() },
        expiry:agent_automation::ExpiryRule::None,now_ms:chrono::Utc::now().timestamp_millis(),
    }).await?;
    let id = created.definition.wakeup_id;
    store.close().await?;
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
        directory: root.to_owned(),
        codex_home: root.to_owned(),
        backend_socket: root.join("unavailable-native.sock"),
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        native_schema: None,
        peer_registry_directory: Some(root.join("peers")),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?;
    let client = CollaborationClient::connect(root, "legacy-wake-timer-test", "1").await?;
    // Failure guard only: the real first-fire wait answers from retained timer events.
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
    let collaboration_client::WakeFirstFireResponse::Local { value: fired } = fired else {
        return Err("unmoved Wake changed timer ownership".into());
    };
    if fired.wakeup_id != id {
        return Err("timer changed its actual first-fire identity".into());
    }
    let collaboration_client::WakeReadResponse::Local { value: observed } = client
        .read_wakeup(WakeShowRequest { wakeup_id: id })
        .await?
    else {
        return Err("unmoved local Wake changed read ownership".into());
    };
    if observed.first_fire.is_none() || observed.pending_delivery_id.is_none() {
        return Err("firing lost durable first-fire or delivery identity".into());
    }
    drop(client);
    runtime.shutdown().await?;
    directory.close()?;
    Ok(())
}
