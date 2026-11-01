//! Real central custody; target-reported outcomes stand in for native creation only.
use crate::automation_catalog_scenarios::{call, open_machine};
use crate::{AutomationClockControl, ConformanceClient, ConformanceResult};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, MachineWorkProtocol,
    PullAutomationBatches, RemoteSessionCreateRequest,
};
use serde_json::json;

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
        let request = session_create_request(&source_service, &target_service)?;
        let admitted = call(&mut source, AutomationCall::RemoteSessionCreate(request)).await?;
        if !matches!(admitted, AutomationAnswer::SessionCreate(ref row)
            if row.create.is_none() && row.report_seq.is_none()) {
            return Err(format!("central admission must return unreported custody, got {admitted:?}").into());
        }
        let after = client.automation_clock(AutomationClockControl::Observe).await?;
        if after.now != before.now || after.callbacks.len() != before.callbacks.len() {
            return Err("immediate SessionCreate admission invoked an alarm".into());
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let source_closed = source.close().await;
    let target_closed = target.close().await;
    proof?;
    source_closed?;
    target_closed?;
    Ok(())
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
