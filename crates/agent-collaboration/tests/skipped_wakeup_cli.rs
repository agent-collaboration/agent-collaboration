//! Replaying a central one-shot creation observes its skipped current state through the CLI.
#[path = "support/central_automation_host.rs"]
mod fixture;
use collaboration_protocol::OperationId;
use fixture::{CentralHost, ProofResult};
use serde_json::{Value, json};

#[tokio::test]
async fn skipped_one_shot_cli_wait_reports_no_firing() -> ProofResult {
    let host = CentralHost::start(fixture::directory()?).await?;
    let proof: ProofResult = async {
        let target = json!({"endpoint":{"serviceId":host.runtime.service_id(),"endpointId":"codex-local"},"sessionId":"skipped-cli-target"}).to_string();
        let operation = OperationId::generate();
        let due = String::from(host.due.clone());
        let arguments = ["wake","send","--to",&target,"--human-user","--text","Reminder",
            "--at",&due,"--for","2h","--operation-id",operation.as_str()];
        let created = host.json(&arguments).await?;
        let id = created.pointer("/result/record/value/definition/wakeupId").and_then(Value::as_str).ok_or("central one-shot identity missing")?;
        host.json(&["wake","pause","--wakeup-id",id]).await?;
        // This causal clock advance completes the paused due-time scan before resuming.
        host.advance_to_due().await?;
        let resumed = host.json(&["wake","resume","--wakeup-id",id]).await?;
        if resumed.pointer("/result/record/value/automation/value/state") != Some(&json!("finished")) {
            return Err(format!("paused one-shot did not skip its due mark: {resumed}").into());
        }
        let mut replay = arguments.to_vec();
        replay.extend(["--wait-until-first-fire","--json"]);
        let output = host.run_cli(&replay).await?;
        let answer: Value = serde_json::from_slice(&output.stdout)?;
        if output.status.code() != Some(4)
            || answer.pointer("/created/kind") != Some(&json!("central"))
            || answer.pointer("/created/value/definition/wakeupId") != Some(&json!(id))
            || answer.pointer("/wait/wakeupId") != Some(&json!(id))
            || answer.pointer("/wait/outcome/kind") != Some(&json!("finishedWithoutFiring")) {
            return Err(format!("CLI lost the durable creation or terminal skipped result: {answer}").into());
        }
        let current = host.json(&["wake","show","--wakeup-id",id]).await?;
        if current.pointer("/result/record/value/state") != Some(&json!("finished"))
            || current.pointer("/result/record/value/firstFiring") != Some(&Value::Null)
            || current.pointer("/result/record/value/firstFire").is_some()
            || current.pointer("/result/record/value/nextDueAt") != Some(&Value::Null) {
            return Err("skipped Wake invented firing evidence or future work".into());
        }
        for arguments in [
            vec!["firing","list","--automation-id",id],
            vec!["delivery","list","--wakeup-id",id],
        ] {
            let page = host.json(&arguments).await?;
            if page.pointer("/result/page/records").and_then(Value::as_array).is_none_or(|records| !records.is_empty()) {
                return Err(format!("skipped Wake invented admitted work: {page}").into());
            }
        }
        Ok(())
    }.await;
    let stopped = host.close().await;
    proof?;
    stopped
}
