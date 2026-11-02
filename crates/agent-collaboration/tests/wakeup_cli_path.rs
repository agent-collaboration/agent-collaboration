//! New creation is central; retained Local receipts and delivery history keep their real shape.
#[path = "support/central_automation_host.rs"]
mod fixture;
use collaboration_protocol::{OperationId, WakeShowRequest};
use fixture::{CentralHost, ProofResult};
use serde_json::{Value, json};

#[tokio::test]
async fn cli_creates_wakeup_with_harness_sender_and_reads_through_host() -> ProofResult {
    let host = CentralHost::start(fixture::directory()?).await?;
    let proof: ProofResult = async {
        let target=json!({"endpoint":{"serviceId":host.runtime.service_id(),"endpointId":"codex-local"},"sessionId":"fixture-only-new-thread"}).to_string();
        let created=host.json(&["wake","send","--to",&target,"--every","10m","--for","2h","--text","Check repository"]).await?;
        let id=created.pointer("/result/record/value/definition/wakeupId").and_then(Value::as_str).ok_or("created central Wake identity missing")?;
        if created.pointer("/result/record/kind")!=Some(&json!("central")) { return Err("new CLI Wake changed to Local".into()); }
        let read=host.json(&["wake","show","--wakeup-id",id]).await?;
        if read.pointer("/result/record/value/definition/message/content/text")!=Some(&json!("Check repository")) {
            return Err("CLI wake text was not persisted".into());
        }
        if read.pointer("/result/record/value/definition/message/content/kind")!=Some(&json!("agent"))
            || read.pointer("/result/record/value/definition/message/content/sender/sessionId")!=Some(&json!("wake-cli-sender")) {
            return Err("CLI wake did not persist the harness agent identity".into());
        }
        if read.pointer("/result/record/value/firstFiring")!=Some(&Value::Null)
            || read.pointer("/result/record/value/firstFire").is_some() {
            return Err("CLI creation invented a Firing or local FireReceipt".into());
        }
        for (action,state) in [("pause","paused"),("resume","active"),("cancel","cancelled")] {
            let response=host.json(&["wake",action,"--wakeup-id",id]).await?;
            if response.pointer("/result/record/kind")!=Some(&json!("central"))
                || response.pointer("/result/record/value/automation/value/state")!=Some(&json!(state)) {
                return Err(format!("wake {action} returned wrong central state").into());
            }
        }
        let operation=OperationId::generate();
        let due=String::from(host.due.clone());
        let arguments=["wake","send","--to",&target,"--text","Check after timer","--at",&due,
            "--for","2h","--operation-id",operation.as_str()];
        let created=host.json(&arguments).await?;
        let fired_id=created.pointer("/result/record/value/definition/wakeupId").and_then(Value::as_str).ok_or("one-shot Wake missing")?;
        host.advance_to_due().await?;
        // Replay the same durable creation, then observe its already admitted first Firing.
        let mut waiting=arguments.to_vec(); waiting.push("--wait-until-first-fire");
        let fired=host.json(&waiting).await?;
        let first=fired.pointer("/result/record/value/firstFiring").ok_or("one-result wait omitted actual first Firing")?;
        let firing_id=first.get("firingId").and_then(Value::as_str).ok_or("first Firing identity missing")?;
        if first.get("automationId")!=Some(&json!(fired_id)) || fired.pointer("/result/record/value/firstFire").is_some() {
            return Err("central CLI wait invented a Local receipt or substituted its Wake".into());
        }
        let shown=host.json(&["firing","show","--firing-id",firing_id]).await?;
        for field in ["firingId","automationId","dueAt","revision","content"] {
            if shown.pointer("/result/record").and_then(|record|record.get(field))!=first.get(field) {
                return Err(format!("Firing CLI changed immutable {field}").into());
            }
        }
        let firings=host.json(&["firing","list","--automation-id",fired_id]).await?;
        if firings.pointer("/result/page/records/0/firingId")!=Some(&json!(firing_id)) {
            return Err("Firing list omitted its actual central record".into());
        }
        let listing=host.json(&["wake","list","--limit","1"]).await?;
        if listing.pointer("/result/page/records").and_then(Value::as_array).map(Vec::len)!=Some(1)
            || listing.pointer("/result/page/nextCursor").is_none_or(Value::is_null) {
            return Err("CLI bounded listing missing page or cursor".into());
        }
        // Actual CLI -> Host -> central rejection, retaining the operation and neighbours.
        let invalid_operation=OperationId::generate();
        let rejected=host.run_cli(&["wake","send","--to",&target,"--text","Off-grid refusal",
            "--after","1s","--for","2h","--operation-id",invalid_operation.as_str(),"--json"]).await?;
        let refusal:Value=serde_json::from_slice(&rejected.stdout)?;
        let wire=String::from_utf8(rejected.stdout)?;
        if rejected.status.code()!=Some(4) || refusal.pointer("/error/kind")!=Some(&json!("refused"))
            || refusal.get("operationId")!=Some(&json!(invalid_operation))
            || !["offGrid","earlier","later"].iter().all(|term|wire.contains(term)) {
            return Err(format!("CLI lost typed off-grid refusal or neighbours: {refusal}").into());
        }
        use sqlx::Connection as _;
        let mut sql=sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(host.root().join("automation.sqlite"))).await?;
        let local:i64=sqlx::query_scalar("SELECT count(*) FROM wakeup_definitions WHERE authority='local'").fetch_one(&mut sql).await?;
        sql.close().await?;
        if local!=0 {return Err("new CLI Wakes retained Local timer truth".into());}
        Ok(())
    }.await;
    let stopped = host.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn cli_retains_local_fire_receipt_and_delivery_inspection_after_cutover() -> ProofResult {
    let directory = fixture::directory()?;
    let service = collaboration_runtime::load_service_identity(directory.path())?;
    let mut store =
        automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await?;
    let created=store.create_wakeup(&automation_storage::WakeCreate {
        operation_id:OperationId::generate(),requested_by:collaboration_protocol::WakeCreator::Owner {},replace:false,
        message:serde_json::from_value::<collaboration_protocol::SavedMessage>(json!({"target":{"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"retained-cli-wake"},
            "content":{"kind":"humanUser","text":"Retained Local timer"},"delivery":"auto"}))?,
        timing:agent_automation::TimingRule::At {at:chrono::Utc::now()},
        expiry:agent_automation::ExpiryRule::After {seconds:3600},now_ms:chrono::Utc::now().timestamp_millis(),
    }).await?;
    let id = created.definition.wakeup_id;
    store.close().await?;
    let host = CentralHost::start(directory).await?;
    let proof: ProofResult = async {
        let client = collaboration_client::CollaborationClient::connect(
            host.root(),
            "retained-cli-fire",
            "1",
        )
        .await?;
        let observed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
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
        let collaboration_client::WakeFirstFireResponse::Local { value: observed } = observed
        else {
            return Err("retained timer changed first-fire owner".into());
        };
        let shown = host
            .json(&["wake", "show", "--wakeup-id", id.as_str()])
            .await?;
        if shown.pointer("/result/record/kind") != Some(&json!("local")) {
            return Err("retained CLI read lost Local custody".into());
        }
        let first = shown
            .pointer("/result/record/value/firstFire")
            .ok_or("stored Local first fire missing")?;
        let mut fields = first
            .as_object()
            .ok_or("firstFire is not an object")?
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        fields.sort_unstable();
        if fields != ["dueAt", "firedAt", "kind", "occurrenceId", "wakeupId"] {
            return Err("firstFire changed the actual FireReceipt shape".into());
        }
        let receipt: collaboration_protocol::FireReceipt = serde_json::from_value(first.clone())?;
        if serde_json::to_value(&receipt)? != *first
            || serde_json::to_value(&observed)? != *first
            || receipt.wakeup_id != id
            || first.get("kind") != Some(&json!("wakeFired"))
        {
            return Err(
                "CLI stored FireReceipt differs from the real first-fire observation".into(),
            );
        }
        let deliveries = host
            .json(&["delivery", "list", "--wakeup-id", id.as_str()])
            .await?;
        let delivery_id = deliveries
            .pointer("/result/page/records/0/deliveryId")
            .and_then(Value::as_str)
            .ok_or("missing actual fired Delivery")?;
        for action in ["show", "attempts", "reconcile"] {
            let output = host
                .json(&["delivery", action, "--delivery-id", delivery_id])
                .await?;
            if action == "attempts"
                && output.pointer("/result/page/coverage/earlierAttempts")
                    != Some(&json!("mayBeUnavailable"))
            {
                return Err("delivery attempts CLI omitted retention coverage".into());
            }
        }
        Ok(())
    }
    .await;
    let stopped = host.close().await;
    proof?;
    stopped
}
