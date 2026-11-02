//! Real CLI, central catalog and retained Local history; no native backend or model execution.
#[path = "support/central_automation_host.rs"]
#[expect(
    dead_code,
    reason = "This target keeps Schedules disabled and does not advance the fixture clock."
)]
mod fixture;
use collaboration_client::CollaborationClient;
use collaboration_protocol::{InstructionCreateParams, InstructionText, OperationId};
use fixture::{CentralHost, ProofResult};
use serde_json::{Value, json};

#[tokio::test]
async fn cli_creates_and_inspects_disabled_schedule() -> ProofResult {
    let directory = fixture::directory()?;
    // The event-history assertion refers to genuine pre-upgrade history, not a shadow write
    // by new central CRUD. This legacy Instruction is not used by the new Schedule.
    let mut legacy =
        automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await?;
    legacy
        .create_instruction(
            &OperationId::generate(),
            &"Retained history before cutover".to_owned().try_into()?,
            chrono::Utc::now().timestamp_millis(),
        )
        .await?;
    legacy.close().await?;
    let host = CentralHost::start(directory).await?;
    let proof:ProofResult=async {
        let client=CollaborationClient::connect(host.root(),"schedule-cli-fixture","1").await?;
        let instruction=client.create_instruction(InstructionCreateParams {
            operation_id:OperationId::generate(),text:InstructionText::try_from("Inspect job status".to_owned())?,
        }).await?;
        let definition=host.root().join("schedule-definition.json");
        std::fs::write(&definition,serde_json::to_vec(&json!({"instructionId":instruction.instruction_id,
            "timing":{"kind":"interval","seconds":600},"enabled":false,"destination":{"kind":"unprepared"},
            "executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"low"}))?)?;
        let definition=definition.to_str().ok_or("definition fixture path")?;
        let created=host.json(&["schedule","create","--definition-file",definition]).await?;
        let id=created.pointer("/result/record/scheduleId").and_then(Value::as_str).ok_or("schedule identity missing")?;
        let read=host.json(&["schedule","show","--schedule-id",id]).await?;
        if read.pointer("/result/record/kind")!=Some(&json!("schedule"))
            || read.pointer("/result/record/value/definition/enabled")!=Some(&json!(false))
            || read.pointer("/result/record/value/activeRunId").is_some()
            || read.pointer("/result/record/value/waitingRunId").is_some() {
            return Err("central Schedule read claimed local occupancy or changed its disabled definition".into());
        }
        let exported=host.run_cli(&["schedule","export","--schedule-id",id]).await?;
        if !exported.status.success() {return Err("schedule CLI export failed".into());}
        let package_file=host.root().join("schedule-package.jsonl");
        std::fs::write(&package_file,&exported.stdout)?;
        let package_file=package_file.to_str().ok_or("package fixture path")?;
        for overwrite in [false,true] {
            let mut arguments=vec!["schedule","import","--package-file",package_file,"--json"];
            if overwrite {arguments.push("--overwrite");}
            let imported=host.run_cli(&arguments).await?;
            if imported.status.success()!=overwrite {return Err("schedule import did not require explicit --overwrite".into());}
            let imported:Value=serde_json::from_slice(&imported.stdout)?;
            if overwrite && (imported.pointer("/result/record/automation/kind")!=Some(&json!("schedule"))
                || imported.pointer("/result/record/automation/value/scheduleId")!=Some(&json!(id))
                || imported.pointer("/result/record/automation/value/definition/enabled")!=Some(&json!(false))
                || imported.pointer("/result/record/alreadySent")!=Some(&json!([]))) {
                return Err("CLI import changed identity, enabled the Schedule or invented sent work".into());
            }
        }
        for (group,parent,count) in [
            ("instruction",None,1),("schedule",None,1),("run",Some(("--schedule-id",id)),0),
            ("revision",Some(("--instruction-id",instruction.instruction_id.as_str())),1),("delivery",None,0),
        ] {
            let mut arguments=vec![group,"list"];
            if let Some((flag,value))=parent {arguments.extend([flag,value]);}
            let page=host.json(&arguments).await?;
            if page.pointer("/result/page/records").and_then(Value::as_array).map(Vec::len)!=Some(count) {
                return Err(format!("{group} list returned wrong records: {page}").into());
            }
        }
        let events=host.json(&["automation","events"]).await?;
        if events.pointer("/result/page/records").and_then(Value::as_array).is_none_or(Vec::is_empty)
            || events.pointer("/result/page/earliestRetainedCursor").and_then(Value::as_str).is_none() {
            return Err("CLI retained event history omitted records or retention cursor".into());
        }
        let root=host.root().to_str().ok_or("owned cwd")?;
        let prepared=host.run_cli(&["schedule","prepare","--schedule-id",id,"--fresh","--cwd",root,"--json"]).await?;
        if prepared.status.success() {return Err("prepare succeeded without a native backend".into());}
        let prepared:Value=serde_json::from_slice(&prepared.stdout)?;
        if prepared.pointer("/error/kind")!=Some(&json!("unsupportedCapability"))
            || prepared.pointer("/error/effects/evidence/allocation")!=Some(&json!("notRequested")) {
            return Err(format!("unavailable prepare did not preserve no-allocation evidence: {prepared}").into());
        }
        use sqlx::Connection as _;
        let mut sql=sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(host.root().join("automation.sqlite"))).await?;
        let local_instruction:i64=sqlx::query_scalar("SELECT count(*) FROM instruction_documents WHERE instruction_id=?").bind(instruction.instruction_id.as_str()).fetch_one(&mut sql).await?;
        let local_schedule:i64=sqlx::query_scalar("SELECT count(*) FROM schedule_definitions WHERE schedule_id=?").bind(id).fetch_one(&mut sql).await?;
        sql.close().await?;
        if local_instruction!=0 || local_schedule!=0 {return Err("CLI central CRUD created Local definitions".into());}
        Ok(())
    }.await;
    let stopped = host.close().await;
    proof?;
    stopped
}
