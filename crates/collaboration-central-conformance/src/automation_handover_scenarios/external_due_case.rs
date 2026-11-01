//! Later timer dues fold while an ordinarily imported external Run is unresolved.
use super::fixtures::{import_request, required_value, required_value_mut, schedule_payload};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::{open_machine, raw_call},
    automation_clock_scenarios::fixture,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationId, FiringEntry, LocalRunOutcome, LocalRunSettled,
    WorkEntry,
};
use serde_json::json;

pub async fn run_automation_handover_external_due_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let importer = durable_steps::Uuid::now_v7().to_string();
    let target = durable_steps::Uuid::now_v7().to_string();
    let mut source = open_machine(client, &importer).await?;
    let mut receiver = match open_machine(client, &target).await {
        Ok(receiver) => receiver,
        Err(error) => {
            source.close().await?;
            return Err(error);
        }
    };
    let proof: ConformanceResult<()> = async {
        let pull = json!({"kind":"pullAutomationBatches","value":{
            "after":null,"maxBytes":1048576,"workProtocols":["machineWorkV1"]
        }});
        if !matches!(raw_call(&mut receiver,pull.clone()).await?,
            AutomationAnswer::Batches(page) if page.batches.is_empty()) {
            return Err("fresh imported external-due target already had a Batch".into());
        }
        let mut payload = schedule_payload(&target);
        *required_value_mut(&mut payload,"/definition/value/timing")? =
            json!({"kind":"interval","seconds":600,"firstDue":"2026-10-31T10:20:00Z"});
        *required_value_mut(&mut payload,"/firstDueMs")? = json!(1793442000000_i64);
        let external_run = durable_steps::Uuid::now_v7().to_string();
        *required_value_mut(&mut payload,"/externalOccupant")? = json!(external_run);
        *required_value_mut(&mut payload,"/carriedWaitingDueMs")? = json!(1793441100000_i64);
        let automation: AutomationId = serde_json::from_value(required_value(&payload,"/automationId")?.clone())?;
        if !matches!(raw_call(&mut source,import_request(&payload.to_string())).await?,
            AutomationAnswer::Imported(imported) if imported.automation_id == automation) {
            return Err("ordinary unresolved external-due import refused its identity".into());
        }
        let imported = fixture::automation(client,&automation,None).await?;
        let pending_json = required_value(&imported,"/firings/0/pendingEntryJson")?
            .as_str().ok_or("imported external waiter missing its frozen entry")?;
        let pending: FiringEntry = serde_json::from_str(pending_json)?;
        if pending.automation_id != automation
            || pending.due_at.unix_millis() != 1793441100000_i64
            || json!(pending.revision) != *required_value(&payload,"/revision")?
            || json!(pending.content.revision_id) != *required_value(&payload,"/content/revisionId")?
            || serde_json::to_value(&pending.work)? != json!({"kind":"schedule","definition":required_value(&payload,"/definition/value")?})
            || *required_value(&imported,"/externalRunId")? != external_run
            || *required_value(&imported,"/waitingFiringId")? != json!(pending.firing_id)
            || !required_value(&imported,"/occupyingFiringId")?.is_null() {
            return Err("ordinary import lost the external Run or carried waiter".into());
        }
        for (time, folded, next) in [
            ("2026-10-31T10:25:00Z",1793442000000_i64,1793442600000_i64),
            ("2026-10-31T10:35:00Z",1793442600000_i64,1793443200000_i64),
        ] {
            let now = RegistryTime(time.parse()?);
            client.automation_clock(AutomationClockControl::Advance { now }).await?;
            client.automation_clock(AutomationClockControl::InvokeAlarm).await?;
            let row = fixture::automation(client,&automation,None).await?;
            let clock = client.automation_clock(AutomationClockControl::Observe).await?;
            println!("HANDOVER_EXTERNAL_LATER_DUE_OBSERVED {}",json!({"clock":clock,"row":row,"externalRun":external_run,"beforeSettlement":true}));
            if clock.now != now
                || *required_value(&row,"/externalRunId")? != external_run
                || *required_value(&row,"/waitingFiringId")? != json!(pending.firing_id)
                || !required_value(&row,"/occupyingFiringId")?.is_null()
                || required_value(&row,"/firings")?.as_array().map(Vec::len) != Some(1)
                || *required_value(&row,"/firings/0/firingId")? != json!(pending.firing_id)
                || *required_value(&row,"/firings/0/dueMs")? != 1793441100000_i64
                || *required_value(&row,"/firings/0/state")? != "pending"
                || *required_value(&row,"/firings/0/foldedThroughMs")? != folded
                || *required_value(&row,"/firings/0/pendingEntryJson")? != pending_json
                || !required_value(&row,"/firings/0/batchId")?.is_null()
                || required_value(&row,"/batches")?.as_array().map(Vec::len) != Some(0)
                || *required_value(&row,"/counts")? != *required_value(&imported,"/counts")?
                || *required_value(&row,"/admittedThroughMs")? != folded
                || *required_value(&row,"/nextDueMs")? != next {
                return Err("unresolved imported external Run admitted a successor/Batch or lost its folded waiter after a later due".into());
            }
            if !matches!(raw_call(&mut receiver,pull.clone()).await?,
                AutomationAnswer::Batches(page) if page.batches.is_empty()) {
                return Err("unresolved imported external Run dispatched before settlement".into());
            }
            println!("HANDOVER_EXTERNAL_LATER_DUE_UNRESOLVED {}",json!({"now":clock.now,"automationId":automation,"externalRun":external_run,"waitingFiringId":pending.firing_id,"foldedThroughMs":folded,"nextDueMs":next,"firings":1,"batches":0,"pulledBatches":0}));
        }
        let settle = serde_json::to_value(AutomationCall::LocalRunSettled(LocalRunSettled {
            automation_id:automation.clone(),run_id:external_run.try_into()?,outcome:LocalRunOutcome::Finished,
        }))?;
        if !matches!(raw_call(&mut source,settle.clone()).await?,AutomationAnswer::Acknowledged) {
            return Err("external Run settlement refused after later dues".into());
        }
        let released = fixture::automation(client,&automation,None).await?;
        if !required_value(&released,"/externalRunId")?.is_null()
            || !required_value(&released,"/waitingFiringId")?.is_null()
            || *required_value(&released,"/occupyingFiringId")? != json!(pending.firing_id)
            || required_value(&released,"/firings")?.as_array().map(Vec::len) != Some(1)
            || *required_value(&released,"/firings/0/firingId")? != json!(pending.firing_id)
            || *required_value(&released,"/firings/0/foldedThroughMs")? != 1793442600000_i64
            || !required_value(&released,"/firings/0/pendingEntryJson")?.is_null()
            || *required_value(&released,"/nextDueMs")? != 1793443200000_i64 {
            return Err("post-due settlement lost the same owed waiter or future progress".into());
        }
        let AutomationAnswer::Batches(page) = raw_call(&mut receiver,pull.clone()).await? else {
            return Err("post-settlement Batch pull refused".into());
        };
        let batch = page.batches.first().ok_or("settled external waiter emitted no Batch")?;
        let frozen: collaboration_protocol::AutomationBatch = serde_json::from_slice(&batch.frozen_bytes)?;
        let Some(WorkEntry::Firing(entry)) = frozen.entries.entries().first() else {
            return Err("settled external waiter Batch has no Firing".into());
        };
        if page.batches.len() != 1 || frozen.entries.entries().len() != 1
            || serde_json::to_value(entry)? != serde_json::to_value(&pending)?
            || *required_value(&released,"/firings/0/batchId")? != json!(batch.batch_id) {
            return Err("settled external Run did not release exactly its original owed entry".into());
        }
        let duplicate = raw_call(&mut source,settle).await?;
        if !matches!(duplicate,AutomationAnswer::Acknowledged) {
            return Err("duplicate post-due settlement refused".into());
        }
        client.automation_clock(AutomationClockControl::InvokeAlarm).await?;
        let AutomationAnswer::Batches(repeated) = raw_call(&mut receiver,pull).await? else {
            return Err("duplicate post-due Batch pull refused".into());
        };
        let final_row = fixture::automation(client,&automation,None).await?;
        if serde_json::to_value(&repeated.batches)? != serde_json::to_value(&page.batches)?
            || *required_value(&final_row,"/counts")? != *required_value(&released,"/counts")?
            || required_value(&final_row,"/firings")?.as_array().map(Vec::len) != Some(1) {
            return Err("duplicate post-due settlement or callback minted successor work".into());
        }
        println!("HANDOVER_EXTERNAL_LATER_DUE_RESULT {}",json!({"released":released,"retainedEntry":pending,"batch":batch,"duplicateStable":true}));
        Ok(())
    }.await;
    let source_closed = source.close().await;
    let receiver_closed = receiver.close().await;
    proof?;
    source_closed?;
    receiver_closed?;
    Ok(())
}
