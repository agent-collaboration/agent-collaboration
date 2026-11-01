//! Real central custody; target-reported outcomes stand in for native creation only.
use crate::automation_catalog_scenarios::{call, open_machine};
use crate::{AutomationClockControl, ConformanceClient, ConformanceResult};
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationCall, AutomationRefusal, FirstInputState,
    MachineWorkCustodyState, MachineWorkProtocol, PullAutomationBatches,
    RemoteSessionCreateRequest, WorkEntry,
};
use serde_json::json;
mod corruption_cases;
mod fixture;
mod mixed_report_cases;
mod report_cases;
mod restart_cases;
pub use restart_cases::{
    RemoteCreateRestartCase, prepare_remote_create_restart, resume_remote_create_restart,
};

pub async fn run_remote_session_create_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let source_service = durable_steps::Uuid::now_v7().to_string();
    let target_service = durable_steps::Uuid::now_v7().to_string();
    let mut source = open_machine(client, &source_service).await?;
    let mut target = match open_machine(client, &target_service).await {
        Ok(target) => target,
        Err(error) => {
            source.close().await?;
            return Err(error);
        }
    };
    let proof = async {
        let request = session_create_request(&source_service, &target_service)?;
        let answer = call(&mut source, AutomationCall::RemoteSessionCreate(request)).await?;
        if !matches!(answer, AutomationAnswer::Refused(AutomationRefusal::TargetNeedsUpgrade { ref service_id })
            if String::from(service_id.clone()) == target_service) {
            return Err(format!("unadvertised target must require upgrade, got {answer:?}").into());
        }
        let advertised = call(&mut target, AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None, max_bytes: 1_048_576,
            work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        })).await?;
        if !matches!(advertised, AutomationAnswer::Batches(ref page) if page.batches.is_empty()) {
            return Err("fresh target has unexpected custody".into());
        }
        let before = client.automation_clock(AutomationClockControl::Observe).await?;
        if before.now.0.timestamp_millis() % 300_000 == 0 {
            return Err("remote admission fixture must start off the firing grid".into());
        }
        let request = session_create_request(&source_service, &target_service)?;
        let admitted = match call(&mut source, AutomationCall::RemoteSessionCreate(request.clone())).await? {
            AutomationAnswer::SessionCreate(row) => row,
            other => return Err(format!("central admission omitted committed item identity: {other:?}").into()),
        };
        if admitted.state != MachineWorkCustodyState::Pending
            || admitted.create.is_some() || admitted.report_seq.is_some()
            || !matches!(&admitted.input, Some(FirstInputState::None))
        {
            return Err(format!("central admission fabricated reported facts: {admitted:?}").into());
        }
        let status = call(&mut source, AutomationCall::RemoteSessionCreateStatus {
            operation_id: request.operation_id.clone(),
        }).await?;
        if !matches!(status, AutomationAnswer::SessionCreate(ref row)
            if row.work_item_id == admitted.work_item_id && row.operation_id == request.operation_id
                && row.create.is_none() && row.report_seq.is_none()
                && matches!(&row.input, Some(FirstInputState::None)))
        {
            return Err(format!("fresh status did not read the committed admission: {status:?}").into());
        }
        let batches = pull_work(&mut target).await?;
        if batches.batches.len() != 1 {
            return Err("one admission must commit exactly one immediate Batch".into());
        }
        let frozen = batches.batches.first().ok_or("remote Batch missing")?;
        let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
        if batch.entries.entries().len() != 1 {
            return Err("remote create Batch must contain one entry".into());
        }
        let Some(WorkEntry::SessionCreate(entry)) = batch.entries.entries().first() else {
            return Err("remote create Batch omitted its SessionCreate entry".into());
        };
        if entry.work_item_id != admitted.work_item_id || entry.operation_id != request.operation_id
            || entry.source_installation.as_str() != source_service
            || String::from(batch.service_id) != target_service
            || serde_json::to_vec(&entry.request)? != serde_json::to_vec(&request.definition)?
        {
            return Err("central changed the frozen request or verified provenance".into());
        }
        let replay = call(&mut source, AutomationCall::RemoteSessionCreate(request.clone())).await?;
        if !matches!(replay, AutomationAnswer::SessionCreate(ref row)
            if row.work_item_id == admitted.work_item_id)
        {
            return Err("operation replay replaced the committed work item".into());
        }
        let again = pull_work(&mut target).await?;
        if serde_json::to_value(&again.batches)? != serde_json::to_value(&batches.batches)? {
            return Err("operation replay duplicated or changed the immutable Batch".into());
        }
        let mut changed = request.clone();
        changed.definition.working_directory = "/different-request".to_owned().try_into()?;
        for answer in [
            call(&mut source, AutomationCall::RemoteSessionCreate(changed)).await?,
            call(&mut target, AutomationCall::RemoteSessionCreate(request.clone())).await?,
        ] {
            if !matches!(answer, AutomationAnswer::Refused(AutomationRefusal::OperationConflict { ref operation_id })
                if *operation_id == request.operation_id)
            {
                return Err(format!("operation replay failed to bind full request and verified source: {answer:?}").into());
            }
        }
        let after = client.automation_clock(AutomationClockControl::Observe).await?;
        if after.now != before.now {
            return Err("remote admission advanced the controlled business clock".into());
        }
        report_cases::run(client, &mut source, &mut target, &request, &admitted, frozen).await?;
        println!("REMOTE_CREATE_ADMISSION_RESULT {}", json!({
            "workItemId":admitted.work_item_id,"operationId":request.operation_id,
            "batchId":frozen.batch_id,"sameOffGridTime":before.now,
            "freshStatusCommitted":true,"immutableReplay":true,
            "beforeCallbackCount":before.callbacks.len(),"afterCallbackCount":after.callbacks.len()
        }));
        corruption_cases::run(client,&mut source,&mut target,&source_service,&target_service).await?;
        mixed_report_cases::run(client,&mut source,&mut target,&source_service,&target_service).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let source_closed = source.close().await;
    let target_closed = target.close().await;
    proof?;
    source_closed?;
    target_closed?;
    Ok(())
}

async fn pull_work(
    target: &mut crate::native_board_session::NativeBoardSession,
) -> ConformanceResult<collaboration_protocol::AutomationBatches> {
    match call(
        target,
        AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None,
            max_bytes: 1_048_576,
            work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        }),
    )
    .await?
    {
        AutomationAnswer::Batches(page) => Ok(page),
        other => Err(format!("target custody pull refused: {other:?}").into()),
    }
}

fn session_create_request(
    source: &str,
    target: &str,
) -> ConformanceResult<RemoteSessionCreateRequest> {
    Ok(serde_json::from_value(json!({
        "operationId":durable_steps::Uuid::now_v7().to_string(),
        "definition":{
            "endpoint":{"serviceId":target,"endpointId":"codex-local"},
            "workingDirectory":"/fixture",
            "createdBy":{"endpoint":{"serviceId":source,"endpointId":"codex-local"},"sessionId":"requester"},
            "approver":null,"requestedPolicy":{"access":"workspace-write"},
            "settings":null,"firstInput":null
        }
    }))?)
}
