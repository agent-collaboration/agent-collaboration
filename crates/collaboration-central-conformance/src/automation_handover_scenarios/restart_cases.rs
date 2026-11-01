//! Driver facts cross owning-store reopen; imported work remains owned by Fleet.
use super::{
    fixtures::{import_request, required_value, required_value_mut, schedule_payload},
    receipt_cases::{require_record, unchanged},
    wake_cases::{accept_report, report_request, require_received, wake_payload},
};
use crate::{
    AnswerFaultObservation, AutomationClockControl, ConformanceClient, ConformanceResult,
    RuntimeControl,
    automation_catalog_scenarios::{open_machine, raw_call},
    automation_clock_scenarios::fixture,
    composed_client, enroll_admitted_peer,
    native_board_session::NativeBoardSession,
};
use board_sync_protocol::{BoardConsumerRequest, BoardPayload};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_protocol::{
    AutomationAnswer, AutomationId, FiringEntry, FiringId, FiringOutcome, WorkEntry,
};
use fleet_sync_protocol::{FrameId, OperationRequest};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub struct AutomationHandoverRestartCase {
    importer: String,
    importer_peer: String,
    target: String,
    target_peer: String,
    schedule_payload: Value,
    schedule_request: Value,
    wake_payload: Value,
    counts_before: Value,
    now: RegistryTime,
    dropped_frame: FrameId,
}

pub async fn prepare_automation_handover_restart(
    client: &ConformanceClient,
    waiting_due_now: bool,
) -> ConformanceResult<AutomationHandoverRestartCase> {
    let importer = durable_steps::Uuid::now_v7().to_string();
    let target = durable_steps::Uuid::now_v7().to_string();
    let mut source = open_machine(client, &importer).await?;
    let receiver = match open_machine(client, &target).await {
        Ok(receiver) => receiver,
        Err(error) => {
            source.close().await?;
            return Err(error);
        }
    };
    let proof: ConformanceResult<AutomationHandoverRestartCase> = async {
        let now = RegistryTime(if waiting_due_now { "2026-10-31T10:05:00Z" } else { "2026-10-31T10:00:00Z" }.parse()?);
        if waiting_due_now { client.automation_clock(AutomationClockControl::Advance { now }).await?; }
        let mut payload = schedule_payload(&target);
        *required_value_mut(&mut payload, "/definition/value/timing")? = json!({"kind":"at","at":"2026-10-31T10:20:00Z"});
        *required_value_mut(&mut payload, "/firstDueMs")? = json!(1793442000000_i64);
        *required_value_mut(&mut payload, "/externalOccupant")? = json!(durable_steps::Uuid::now_v7().to_string());
        *required_value_mut(&mut payload, "/carriedWaitingDueMs")? = json!(1793441100000_i64);
        let schedule: AutomationId = serde_json::from_value(required_value(&payload, "/automationId")?.clone())?;
        let counts_before = required_value(&fixture::automation(client, &schedule, None).await?, "/counts")?.clone();
        let mut wake = wake_payload(&target)?;
        *required_value_mut(&mut wake, "/definition/value/configuration/state")? = json!("paused");
        let imported = raw_call(&mut source, import_request(&serde_json::to_string_pretty(&wake)?)).await?;
        if serde_json::to_value(imported)? != json!({"kind":"imported","value":{"automationId":required_value(&wake,"/automationId")?}}) {
            return Err("restart carried Wake did not import its supplied identity".into());
        }
        let request = import_request(&serde_json::to_string_pretty(&payload)?);
        let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        let connection = ObjectAddress::Connection(importer.clone().try_into()?);
        composed_client::control(client, connection.clone(), RuntimeControl::DropNextAnswer { frame }).await?;
        composed_client::send(&mut source.0.socket, frame,
            OperationRequest::Consumer(BoardConsumerRequest::Automation(BoardPayload::encode(&request)?))).await?;
        let observed = composed_client::control(client, connection, RuntimeControl::AwaitDroppedAnswer { frame }).await?;
        if !matches!(observed.answer, AnswerFaultObservation::Dropped { frame: actual } if actual == frame) {
            return Err("handoverCentralCommittedBeforeReply did not drop the actual frame".into());
        }
        let clock = client.automation_clock(AutomationClockControl::Observe).await?;
        if clock.now != now {
            return Err("restart preparation changed its controlled time".into());
        }
        println!("HANDOVER_CENTRAL_COMMITTED_BEFORE_REPLY {}", json!({"frame":frame,"automationId":schedule,"waitingDueNow":waiting_due_now,"clock":clock,"noImportRetry":true}));
        Ok(AutomationHandoverRestartCase {
            importer, importer_peer:source.0.peer.to_string(), target, target_peer:receiver.0.peer.to_string(),
            schedule_payload:payload, schedule_request:request, wake_payload:wake, counts_before, now, dropped_frame:frame,
        })
    }.await;
    let source_closed = source.close().await;
    let receiver_closed = receiver.close().await;
    let prepared = proof?;
    source_closed?;
    receiver_closed?;
    Ok(prepared)
}

pub async fn resume_automation_handover_restart(
    client: &ConformanceClient,
    prepared: AutomationHandoverRestartCase,
) -> ConformanceResult<()> {
    // Passive observations precede reconnect, retry, settlement and every business call.
    let clock = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    let schedule: AutomationId = serde_json::from_value(
        required_value(&prepared.schedule_payload, "/automationId")?.clone(),
    )?;
    let wake: AutomationId =
        serde_json::from_value(required_value(&prepared.wake_payload, "/automationId")?.clone())?;
    let reopened = fixture::automation(client, &schedule, None).await?;
    let reopened_wake = fixture::automation(client, &wake, None).await?;
    println!(
        "HANDOVER_REOPEN_OBSERVED_BEFORE_RETRY {}",
        json!({"droppedFrame":prepared.dropped_frame,"schedule":reopened,"wake":reopened_wake,"clock":clock})
    );
    let frozen = required_value(&prepared.schedule_request, "/value/payload")?
        .as_str()
        .ok_or("frozen import bytes missing")?;
    require_record(
        &reopened,
        &prepared.schedule_payload,
        &prepared.importer,
        frozen,
        required_value(&prepared.schedule_request, "/value/importId")?,
    )?;
    require_imported_counts(
        &prepared.counts_before,
        required_value(&reopened, "/counts")?,
    )?;
    let pending: FiringEntry = serde_json::from_str(
        required_value(&reopened, "/firings/0/pendingEntryJson")?
            .as_str()
            .ok_or("reopened retained entry absent")?,
    )?;
    if pending.automation_id != schedule
        || pending.due_at.unix_millis() != 1793441100000_i64
        || required_value(&reopened, "/firings")?
            .as_array()
            .map(Vec::len)
            != Some(1)
        || *required_value(&reopened, "/externalRunId")?
            != *required_value(&prepared.schedule_payload, "/externalOccupant")?
        || *required_value(&reopened, "/waitingFiringId")? != json!(pending.firing_id)
        || !required_value(&reopened, "/occupyingFiringId")?.is_null()
        || !required_value(&reopened, "/firings/0/batchId")?.is_null()
        || *required_value(&reopened, "/firings/0/state")? != "pending"
        || *required_value(&prepared.schedule_payload, "/revision")? != json!(pending.revision)
        || *required_value(&prepared.schedule_payload, "/content/revisionId")?
            != json!(pending.content.revision_id)
    {
        return Err("reopen lost imported external occupancy or its retained pending work".into());
    }
    require_received(&reopened_wake, &prepared.wake_payload, &prepared.importer)?;
    if clock.now != prepared.now
        || clock.platform_alarm_before_control_ms != Some(1793442000000_i64)
        || clock.next_alarm_ms != Some(1793442000000_i64)
    {
        return Err("owning-store reopen lost its retained clock or platform deadline".into());
    }
    println!(
        "HANDOVER_REOPEN_BEFORE_RETRY {}",
        json!({"droppedFrame":prepared.dropped_frame,"schedule":reopened,"wake":fixture::automation(client,&wake,None).await?,"pendingEntry":pending,"clock":clock})
    );
    let mut source = reconnect(client, &prepared.importer, &prepared.importer_peer).await?;
    let mut target = match reconnect(client, &prepared.target, &prepared.target_peer).await {
        Ok(target) => target,
        Err(error) => {
            source.close().await?;
            return Err(error);
        }
    };
    let proof = after_reopen(
        client,
        &mut source,
        &mut target,
        &prepared,
        &schedule,
        &wake,
        &pending,
    )
    .await;
    let source_closed = source.close().await;
    let target_closed = target.close().await;
    proof?;
    source_closed?;
    target_closed?;
    Ok(())
}

async fn after_reopen(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    prepared: &AutomationHandoverRestartCase,
    schedule: &AutomationId,
    wake: &AutomationId,
    original_pending: &FiringEntry,
) -> ConformanceResult<()> {
    if !pull_batches(target).await?.is_empty() {
        return Err("reopened external or carried work dispatched before settlement".into());
    }
    let replay = json!({"kind":"alreadyImported","value":{"automationId":schedule}});
    unchanged(
        client,
        source,
        schedule,
        prepared.schedule_request.clone(),
        &replay,
    )
    .await?;
    let updated_text = "validated pre-cutoff edit belongs to retained waiting work";
    let edited=raw_call(source,json!({"kind":"instructionUpdate","value":{
        "operationId":durable_steps::Uuid::now_v7().to_string(),
        "instructionId":required_value(&prepared.schedule_payload,"/definition/value/instructionId")?,
        "expectedRevisionId":required_value(&prepared.schedule_payload,"/content/revisionId")?,
        "text":updated_text
    }})).await?;
    let AutomationAnswer::Instruction(updated_instruction) = edited else {
        return Err(format!("normal edit of imported instruction refused: {edited:?}").into());
    };
    if updated_instruction.text.as_str() != updated_text
        || json!(updated_instruction.revision_id)
            == *required_value(&prepared.schedule_payload, "/content/revisionId")?
    {
        return Err("instruction edit did not commit its new text and revision".into());
    }
    let before_definition_edit = fixture::automation(client, schedule, None).await?;
    if serde_json::from_str::<Value>(
        required_value(&before_definition_edit, "/firings/0/pendingEntryJson")?
            .as_str()
            .ok_or("admitted entry missing after head edit")?,
    )? != serde_json::to_value(original_pending)?
    {
        return Err("instruction-head edit rewrote an already-admitted revision without a Schedule mutation".into());
    }
    let mut definition = required_value(&prepared.schedule_payload, "/definition/value")?.clone();
    *required_value_mut(&mut definition, "/timing")? =
        json!({"kind":"at","at":"2026-10-31T10:30:00Z"});
    *required_value_mut(&mut definition, "/effort")? = json!("high");
    let edited=raw_call(source,json!({"kind":"scheduleUpdate","value":{
        "operationId":durable_steps::Uuid::now_v7().to_string(),"scheduleId":schedule,
        "expectedChangeId":required_value(&prepared.schedule_payload,"/revision")?,"definition":definition
    }})).await?;
    let AutomationAnswer::Mutation {
        automation: collaboration_protocol::CatalogAutomation::Schedule(ref updated),
        ref already_sent,
    } = edited
    else {
        return Err(format!("normal edit of imported Schedule refused: {edited:?}").into());
    };
    if updated.schedule_id.as_str() != schedule.as_str()
        || serde_json::to_value(&updated.definition)? != definition
        || json!(updated.change_id) == *required_value(&prepared.schedule_payload, "/revision")?
        || !already_sent.is_empty()
    {
        return Err(
            "Schedule mutation lost the new definition/revision or dispatched pending work".into(),
        );
    }
    let edited_row = fixture::automation(client, schedule, None).await?;
    let pending: FiringEntry = serde_json::from_str(
        required_value(&edited_row, "/firings/0/pendingEntryJson")?
            .as_str()
            .ok_or("edited retained entry missing")?,
    )?;
    if pending.firing_id != original_pending.firing_id
        || pending.automation_id != original_pending.automation_id
        || pending.due_at != original_pending.due_at
        || pending.revision != updated.change_id
        || pending.content.revision_id != updated_instruction.revision_id
        || pending.content.digest.as_str()
            != format!("{:x}", Sha256::digest(updated_text.as_bytes()))
        || serde_json::to_value(&pending.work)?
            != json!({"kind":"schedule","definition":definition})
        || !required_value(&edited_row, "/firings/0/batchId")?.is_null()
    {
        return Err("pre-cutoff edit lost pending identity/due or failed to replace its content/revision/settings".into());
    }
    fixture::automation(
        client,
        schedule,
        Some("{invalid live body after owning-store reopen".into()),
    )
    .await?;
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    if !required_value(
        &fixture::automation(client, schedule, None).await?,
        "/faultJson",
    )?
    .is_string()
    {
        return Err("later live body did not fault through the real callback".into());
    }
    let settlement =
        serde_json::to_value(collaboration_protocol::AutomationCall::LocalRunSettled(
            collaboration_protocol::LocalRunSettled {
                automation_id: schedule.clone(),
                run_id: serde_json::from_value(
                    required_value(&prepared.schedule_payload, "/externalOccupant")?.clone(),
                )?,
                outcome: collaboration_protocol::LocalRunOutcome::Finished,
            },
        ))?;
    let settled = raw_call(source, settlement.clone()).await?;
    if !matches!(settled, AutomationAnswer::Acknowledged) {
        return Err(format!("reopened external settlement refused: {settled:?}").into());
    }
    if prepared.now.0.timestamp_millis() < pending.due_at.unix_millis() {
        let promoted = fixture::automation(client, schedule, None).await?;
        if !required_value(&promoted, "/firings/0/batchId")?.is_null()
            || *required_value(&promoted, "/occupyingFiringId")? != json!(pending.firing_id)
            || !required_value(&promoted, "/externalRunId")?.is_null()
            || !required_value(&promoted, "/waitingFiringId")?.is_null()
            || client
                .automation_clock(AutomationClockControl::Observe)
                .await?
                .next_alarm_ms
                != Some(pending.due_at.unix_millis())
        {
            return Err("reopened future waiter froze early or lost its own owed deadline".into());
        }
        client
            .automation_clock(AutomationClockControl::InvokeAlarm)
            .await?;
        if !required_value(
            &fixture::automation(client, schedule, None).await?,
            "/firings/0/batchId",
        )?
        .is_null()
        {
            return Err("early callback released reopened future waiter".into());
        }
        client
            .automation_clock(AutomationClockControl::Advance {
                now: RegistryTime("2026-10-31T10:05:00Z".parse()?),
            })
            .await?;
    }
    let released = fixture::automation(client, schedule, None).await?;
    if required_value(&released, "/firings")?
        .as_array()
        .map(Vec::len)
        != Some(1)
        || !required_value(&released, "/firings/0/batchId")?.is_string()
        || !required_value(&released, "/firings/0/pendingEntryJson")?.is_null()
        || !required_value(&released, "/faultJson")?.is_string()
    {
        return Err("reopened owed waiter did not release once despite live fault".into());
    }
    let batches = pull_batches(target).await?;
    if batches.len() != 1 {
        return Err("reopened retained work did not have exactly one Batch".into());
    }
    let batch = batches.first().ok_or("released Batch missing")?;
    let decoded: collaboration_protocol::AutomationBatch =
        serde_json::from_slice(&batch.frozen_bytes)?;
    let Some(WorkEntry::Firing(entry)) = decoded.entries.entries().first() else {
        return Err("released Firing entry missing".into());
    };
    if decoded.entries.entries().len() != 1
        || serde_json::to_value(entry)? != serde_json::to_value(&pending)?
    {
        return Err("reopened Batch recaptured later live content, revision or settings".into());
    }
    let duplicate = raw_call(source, settlement).await?;
    if !matches!(duplicate, AutomationAnswer::Acknowledged) {
        return Err(format!("duplicate reopened settlement refused: {duplicate:?}").into());
    }
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    let repeated = pull_batches(target).await?;
    if serde_json::to_value(&repeated)? != serde_json::to_value(&batches)? {
        return Err("reopened duplicate settlement changed Batch identity or bytes".into());
    }
    let firing: FiringId = serde_json::from_value(
        required_value(&prepared.wake_payload, "/carriedFiring/firingId")?.clone(),
    )?;
    let report = report_request(&firing, 1, FiringOutcome::Delivered)?;
    unchanged(
        client,
        target,
        wake,
        report.clone(),
        &json!({"kind":"refused","value":{"kind":"originMismatch"}}),
    )
    .await?;
    accept_report(source, &firing, 1, report).await?;
    let received = fixture::automation(client, wake, None).await?;
    if *required_value(&received, "/firings/0/reportSeq")? != 1
        || !required_value(&received, "/firings/0/batchId")?.is_null()
        || *required_value(&received, "/firings/0/reportServiceId")? != prepared.importer
        || required_value(&received, "/firings")?
            .as_array()
            .map(Vec::len)
            != Some(1)
        || required_value(&received, "/batches")?
            .as_array()
            .map(Vec::len)
            != Some(1)
    {
        return Err("reopen lost carried Wake report ownership or invented redispatch".into());
    }
    unchanged(
        client,
        source,
        schedule,
        prepared.schedule_request.clone(),
        &replay,
    )
    .await?;
    println!(
        "HANDOVER_RESTART_RESULT {}",
        json!({"nowAtReopen":prepared.now,"released":released,"retainedEntry":pending,"batch":batch,"reportedWake":received,"replayNoWrites":true,"nativeEffects":"central custody only"})
    );
    Ok(())
}

async fn pull_batches(
    machine: &mut NativeBoardSession,
) -> ConformanceResult<Vec<collaboration_protocol::FrozenAutomationBatch>> {
    match raw_call(machine,json!({"kind":"pullAutomationBatches","value":{"after":null,"maxBytes":1048576,"workProtocols":["machineWorkV1"]}})).await?{
        AutomationAnswer::Batches(page)=>Ok(page.batches),other=>Err(format!("restart Batch pull refused: {other:?}").into()),
    }
}

fn require_imported_counts(before: &Value, after: &Value) -> ConformanceResult<()> {
    for (name, delta) in [
        ("automations", 2_u64),
        ("timing", 2),
        ("firings", 2),
        ("batches", 0),
        ("imports", 2),
        ("contentRevisions", 2),
        ("instructionDocuments", 1),
    ] {
        let old = required_value(before, &format!("/{name}"))?
            .as_u64()
            .ok_or("pre-import count missing")?;
        let actual = required_value(after, &format!("/{name}"))?
            .as_u64()
            .ok_or("reopened import count missing")?;
        if actual != old + delta {
            return Err(
                format!("reopen retained an incorrect {name} count before any retry").into(),
            );
        }
    }
    Ok(())
}

async fn reconnect(
    client: &ConformanceClient,
    service: &str,
    peer: &str,
) -> ConformanceResult<NativeBoardSession> {
    Ok(NativeBoardSession(
        enroll_admitted_peer(
            client,
            service,
            peer,
            &format!("automation-catalog-{service}"),
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?,
    ))
}
