//! New Wakes use central even when their requester and target are on different machines.
use super::central_catalog_fixture::{
    CatalogHosts, ProofResult, central_wake_created, connected_transport, future_wake_time,
    machine_work_ready,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{AutomationAnswer, AutomationCall, OperationId, WakeShowRequest};
use collaboration_runtime::CentralAutomationTransport;

#[tokio::test]
async fn public_wake_create_uses_central_without_local_timer_definitions() -> ProofResult {
    create_catalog_wake(false).await
}

#[tokio::test]
async fn public_wake_create_for_another_machine_uses_the_same_central_owner() -> ProofResult {
    create_catalog_wake(true).await
}

#[tokio::test]
async fn public_wake_read_on_another_host_uses_the_central_snapshot_without_a_local_cursor()
-> ProofResult {
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        let transport = connected_transport(&hosts.second).await?;
        machine_work_ready(&transport).await?;
        drop(connected_transport(&hosts.first).await?);
        let first = CollaborationClient::connect(&hosts.first_root, "wake-read-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "wake-read-b", "1").await?;
        let central = first.send_wakeup(serde_json::from_value(serde_json::json!({
            "operationId":OperationId::generate(),"requestedBy":{"kind":"router"},"replace":false,
            "message":{"target":{"endpoint":{"serviceId":second.identity().service_id,"endpointId":"codex-local"},"sessionId":"inert-central-read-wake"},
                "content":{"kind":"router","text":"Future read fixture; no native input"},"delivery":"queue"},
            "timing":{"kind":"at","at":future_wake_time()?},"expiry":{"kind":"after","seconds":7200}
        }))?).await?;
        let central = central_wake_created(central)?;
        // Act: B has no local row for this ID and must read central, never legacy remote mail.
        let shown = second.read_wakeup(WakeShowRequest { wakeup_id: central.definition.wakeup_id.clone() }).await?;
        if serde_json::to_value(shown)? != serde_json::json!({"kind":"central","value":central}) {
            return Err("public central Wake read changed its shared snapshot or invented local cursor/fire fields".into());
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn public_wake_list_merges_local_and_central_with_service_scoped_continuation() -> ProofResult
{
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        let transport = connected_transport(&hosts.second).await?;
        machine_work_ready(&transport).await?;
        let first_transport = connected_transport(&hosts.first).await?;
        machine_work_ready(&first_transport).await?;
        let first = CollaborationClient::connect(&hosts.first_root, "wake-list-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "wake-list-b", "1").await?;
        let mut store = automation_storage::AutomationStore::open(&hosts.first_root.join("automation.sqlite")).await?;
        let local = store.create_wakeup(&automation_storage::WakeCreate {
            operation_id: OperationId::generate(), requested_by: collaboration_protocol::WakeCreator::Router {}, replace: false,
            message: serde_json::from_value::<collaboration_protocol::SavedMessage>(serde_json::json!({"target":{"endpoint":{"serviceId":first.identity().service_id,"endpointId":"codex-local"},"sessionId":"inert-local-listed-wake"},
                "content":{"kind":"router","text":"Future local fixture"},"delivery":"queue"}))?,
            timing: agent_automation::TimingRule::At { at: String::from(future_wake_time()?).parse()? },
            expiry: agent_automation::ExpiryRule::After { seconds: 7200 }, now_ms: chrono::Utc::now().timestamp_millis(),
        }).await?;
        store.close().await?;
        let mut expected = std::collections::BTreeSet::from([local.definition.wakeup_id.as_str().to_owned()]);
        for target in ["inert-central-listed-a", "inert-central-listed-b"] {
            let wake = first.send_wakeup(serde_json::from_value(serde_json::json!({
                "operationId":OperationId::generate(),"requestedBy":{"kind":"router"},"replace":false,
                "message":{"target":{"endpoint":{"serviceId":second.identity().service_id,"endpointId":"codex-local"},"sessionId":target},
                    "content":{"kind":"router","text":"Future central fixture"},"delivery":"queue"},
                "timing":{"kind":"at","at":future_wake_time()?},"expiry":{"kind":"after","seconds":7200}
            }))?).await?;
            let wake = central_wake_created(wake)?;
            expected.insert(wake.definition.wakeup_id.as_str().to_owned());
        }
        let instruction = first.create_instruction(collaboration_protocol::InstructionCreateParams {
            operation_id: OperationId::generate(), text: "Mixed catalog page fixture".to_owned().try_into()?,
        }).await?;
        first.create_schedule(serde_json::from_value(serde_json::json!({"operationId":OperationId::generate(),"definition":{
            "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},"enabled":false,
            "destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"low"
        }}))?).await?;
        let mut request = collaboration_protocol::AutomationPageRequest { cursor: None, limit: 1.try_into()? };
        let mut observed = std::collections::BTreeSet::new();
        let mut local_count = 0;
        let mut central_count = 0;
        let mut empty_pages = 0;
        // One local page, followed by central's two Wake pages and its filtered Schedule page.
        for ordinal in 0..4 {
            let page = first.list_wakeups(request.clone()).await?;
            if page.records.is_empty() { empty_pages += 1; }
            for record in page.records {
                let value = serde_json::to_value(record)?;
                match value["kind"].as_str() {
                    Some("local") => { local_count += 1; if value.pointer("/value/latestEventCursor").and_then(serde_json::Value::as_str).is_none_or(str::is_empty) { return Err("local list record lost its actual event cursor".into()); } },
                    Some("central") => central_count += 1,
                    _ => return Err("Wake listing lost ownership tags or included a Schedule".into()),
                }
                let id = value.pointer("/value/definition/wakeupId").and_then(serde_json::Value::as_str).ok_or("listed Wake identity missing")?;
                if !observed.insert(id.to_owned()) { return Err("steady-state Wake listing duplicated an identity".into()); }
            }
            if ordinal == 0 {
                let cursor = page.next_cursor.clone().ok_or("local phase did not continue into central")?;
                let foreign = second.list_wakeups(collaboration_protocol::AutomationPageRequest { cursor: Some(cursor), limit: 1.try_into()? }).await;
                if !matches!(foreign, Err(collaboration_client::WakeClientError::Rejected(error))
                    if matches!(&error.reason, collaboration_protocol::WakeFailureReason::InvalidField { field, .. } if field == "cursor")) {
                    return Err("a combined Wake cursor was accepted on another service".into());
                }
            }
            match (ordinal, page.next_cursor) {
                (0..=2, Some(cursor)) => request.cursor = Some(cursor), (3, None) => {},
                _ => return Err("combined Wake listing ended early or invented another page".into()),
            }
        }
        if observed != expected || local_count != 1 || central_count != 2 || empty_pages != 1 {
            return Err("combined Wake listing lost a local/central definition or filtered cursor".into());
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

async fn create_catalog_wake(target_other_machine: bool) -> ProofResult {
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        let first_transport = connected_transport(&hosts.first).await?;
        let second_transport = connected_transport(&hosts.second).await?;
        for transport in [&first_transport, &second_transport] {
            // Actual machine-session pulls establish capability before the catalog write.
            if !matches!(transport.call(AutomationCall::PullAutomationBatches(collaboration_protocol::PullAutomationBatches {
                after: None, max_bytes: 1_048_576,
                work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
            })).await?, AutomationAnswer::Batches(_)) {
                return Err("Wake fixture capability was not admitted".into());
            }
        }
        let first = CollaborationClient::connect(&hosts.first_root, "wake-catalog-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "wake-catalog-b", "1").await?;
        let target_service = if target_other_machine { &second.identity().service_id } else { &first.identity().service_id };
        let request: collaboration_protocol::WakeSendRequest = serde_json::from_value(serde_json::json!({
            "operationId":OperationId::generate(),"requestedBy":{"kind":"router"},"replace":false,
            "message":{"target":{"endpoint":{"serviceId":target_service,"endpointId":"codex-local"},"sessionId":"inert-future-wake-target"},"content":{"kind":"router","text":"Future fixture only; native backend absent"},"delivery":"queue"},
            "timing":{"kind":"at","at":future_wake_time()?},"expiry":{"kind":"after","seconds":7200}
        }))?;
        // This is future fixture data, with no native process or model input.
        let created = first.send_wakeup(request.clone()).await
            .map_err(|error| format!("public Wake still uses a local or remote-mail timer owner: {error}"))?;
        let created = central_wake_created(created)?;
        let answer = second_transport.call(AutomationCall::WakeShow(WakeShowRequest {
            wakeup_id: created.definition.wakeup_id.clone(),
        })).await?;
        let AutomationAnswer::Wake(central) = answer else {
            return Err("public Wake create never reached central".into());
        };
        let replay = first.send_wakeup(request).await?;
        let replay = central_wake_created(replay)?;
        if serde_json::to_value(&created)? != serde_json::to_value(&central)? || serde_json::to_value(&created)? != serde_json::to_value(replay)? {
            return Err("central Wake replay changed its identity, definition or first-Firing evidence".into());
        }
        if central.definition.message.target.endpoint.service_id != *target_service || central.first_firing.is_some() {
            return Err("central Wake substituted its target or invented a Firing".into());
        }
        use sqlx::Connection;
        for root in [&hosts.first_root, &hosts.second_root] {
            let mut sql = sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(root.join("automation.sqlite"))).await?;
            let local: i64 = sqlx::query_scalar("SELECT count(*) FROM wakeup_definitions").fetch_one(&mut sql).await?;
            let remote: i64 = sqlx::query_scalar("SELECT count(*) FROM remote_wake_requests").fetch_one(&mut sql).await?;
            sql.close().await?;
            if local != 0 || remote != 0 { return Err("public Wake retained a local timer definition or remote-mail shadow".into()); }
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn public_frozen_wake_control_returns_typed_pending_without_local_mutation() -> ProofResult {
    use collaboration_client::{WakeClientError, WakeSelectionFailure};
    use collaboration_protocol::{
        MessageContent, MessageDelivery, SavedMessage, WakeMutationRequest,
    };
    use sqlx::Connection;
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        let client = CollaborationClient::connect(&hosts.first_root, "frozen-wake-control", "1").await?;
        let path = hosts.first_root.join("automation.sqlite");
        let mut seed = automation_storage::AutomationStore::open(&path).await?;
        let local = seed.create_wakeup(&automation_storage::WakeCreate {
            operation_id: OperationId::generate(), requested_by: collaboration_protocol::WakeCreator::Router {}, replace: false,
            message: SavedMessage {
                target: serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":client.identity().service_id,"endpointId":"codex-local"},"sessionId":"inert-frozen-local-wake"}))?,
                content: MessageContent::Router { text: "Local fixture; no native backend".to_owned().try_into()? },
                delivery: MessageDelivery::Queue, generation_guard: None,
            },
            timing: agent_automation::TimingRule::At { at: String::from(future_wake_time()?).parse()? },
            expiry: agent_automation::ExpiryRule::After { seconds: 7200 }, now_ms: chrono::Utc::now().timestamp_millis(),
        }).await?;
        seed.close().await?;
        let id = local.definition.wakeup_id.clone();
        let mut sql = sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(&path)).await?;
        // Only the future producer's marker is seeded; real freeze/import remains an S7 gate.
        sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'pending',0,?,?)")
            .bind(id.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
        let operation = OperationId::generate();
        let result = client.pause_wakeup(WakeMutationRequest { operation_id: operation.clone(), wakeup_id: id.clone() }).await;
        if !matches!(result, Err(WakeClientError::Selection(error)) if matches!(&*error, WakeSelectionFailure::HandoverPending { operation_id, wakeup_id, .. } if operation_id == &operation && wakeup_id == &id)) {
            return Err("public frozen Wake control lost typed handoverPending or caller identities".into());
        }
        let state: String = sqlx::query_scalar("SELECT wakeup_status FROM wakeup_definitions WHERE wakeup_id=?").bind(id.as_str()).fetch_one(&mut sql).await?;
        let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM operation_receipts WHERE operation_id=?").bind(operation.as_str()).fetch_one(&mut sql).await?;
        sql.close().await?;
        if state != "active" || receipts != 0 { return Err("frozen control committed local effects".into()); }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn public_central_wake_controls_preserve_exact_catalog_mutations_across_hosts() -> ProofResult
{
    use collaboration_protocol::WakeMutationRequest;
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        let transport = connected_transport(&hosts.second).await?;
        machine_work_ready(&transport).await?;
        drop(connected_transport(&hosts.first).await?);
        let first = CollaborationClient::connect(&hosts.first_root, "central-wake-control-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "central-wake-control-b", "1").await?;
        let created = first.send_wakeup(serde_json::from_value(serde_json::json!({
            "operationId":OperationId::generate(),"requestedBy":{"kind":"router"},"replace":false,
            "message":{"target":{"endpoint":{"serviceId":second.identity().service_id,"endpointId":"codex-local"},"sessionId":"inert-central-controlled-wake"},"content":{"kind":"router","text":"Future controlled fixture only"},"delivery":"queue"},
            "timing":{"kind":"at","at":future_wake_time()?},"expiry":{"kind":"after","seconds":7200}
        }))?).await?;
        let created = central_wake_created(created)?;
        for action in ["pause", "resume", "cancel"] {
            let request = WakeMutationRequest { operation_id: OperationId::generate(), wakeup_id: created.definition.wakeup_id.clone() };
            let (public, central_call) = match action {
                "pause" => (second.pause_wakeup(request.clone()).await, AutomationCall::WakePause(request)),
                "resume" => (first.resume_wakeup(request.clone()).await, AutomationCall::WakeResume(request)),
                _ => (second.cancel_wakeup(request.clone()).await, AutomationCall::WakeCancel(request)),
            };
            let public = public.map_err(|error| format!("central Wake {action} still used a local/remote-mail owner: {error}"))?;
            let central = transport.call(central_call).await?;
            let expected = serde_json::to_value(central)?.get("value").cloned().ok_or("central Wake mutation missing")?;
            let public = serde_json::to_value(public)?;
            if public.get("kind").and_then(serde_json::Value::as_str) != Some("central") || public.get("value") != Some(&expected) {
                return Err("public Wake control changed central state or actual alreadySent IDs".into());
            }
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}
