//! Carried local delivery is received custody owned by the importing installation.
use super::{
    fixtures::{import_request, required_value, required_value_mut, schedule_payload},
    receipt_cases::unchanged,
};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::{open_machine, raw_call},
    automation_clock_scenarios::fixture,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AutomationAnswer, AutomationId, FiringId, FiringMessageOrigin, FiringOutcome, FiringReport,
    SessionRef, WakeCreator, WorkItemReport, WorkReportSubject,
};
use serde_json::{Value, json};

pub async fn run_automation_handover_wake_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let importer = durable_steps::Uuid::now_v7().to_string();
    let destination = durable_steps::Uuid::now_v7().to_string();
    let mut first = open_machine(client, &importer).await?;
    let mut second = match open_machine(client, &destination).await {
        Ok(second) => second,
        Err(error) => {
            first.close().await?;
            return Err(error);
        }
    };
    let proof: ConformanceResult<()> = async {
        let advertised = raw_call(&mut second, json!({"kind":"pullAutomationBatches","value":{"after":null,"maxBytes":1048576,"workProtocols":["machineWorkV1"]}})).await?;
        if !matches!(advertised, AutomationAnswer::Batches(ref page) if page.batches.is_empty()) {
            return Err("carried Wake target advertisement was not initially empty".into());
        }
        let payload = wake_payload(&destination)?;
        let id: AutomationId = serde_json::from_value((*required_value(&payload, "/automationId")?).clone())?;
        let firing: FiringId = serde_json::from_value((*required_value(&payload, "/carriedFiring/firingId")?).clone())?;
        if !matches!(raw_call(&mut first, import_request(&payload.to_string())).await?, AutomationAnswer::Imported(_)) { return Err("carried Wake import refused".into()); }
        require_received(&fixture::automation(client, &id, None).await?, &payload, &importer)?;
        for time in ["2026-10-31T10:05:00Z", "2026-10-31T10:15:00Z"] {
            client.automation_clock(AutomationClockControl::Advance { now: RegistryTime(time.parse()?) }).await?;
            client.automation_clock(AutomationClockControl::InvokeAlarm).await?;
            require_received(&fixture::automation(client, &id, None).await?, &payload, &importer)?;
            let pulled = raw_call(&mut second, json!({"kind":"pullAutomationBatches","value":{"after":null,"maxBytes":1048576,"workProtocols":["machineWorkV1"]}})).await?;
            if !matches!(pulled, AutomationAnswer::Batches(ref page) if page.batches.is_empty()) { return Err("unresolved carried Wake was dispatched as central work".into()); }
        }
        let report = report_request(&firing, 1, FiringOutcome::Delivered)?;
        unchanged(client, &mut second, &id, report.clone(), &json!({"kind":"refused","value":{"kind":"originMismatch"}})).await?;
        accept_report(&mut first, &firing, 1, report).await?;
        let first_report = fixture::automation(client, &id, None).await?;
        if (*required_value(&first_report, "/firings/0/reportSeq")?) != 1 || serde_json::from_str::<Value>((*required_value(&first_report, "/firings/0/outcomeJson")?).as_str().ok_or("outcome missing")?)? != json!({"state":"delivered"}) { return Err("importer report did not retain its sequence and outcome".into()); }
        accept_report(&mut first, &firing, 3, report_request(&firing, 3, FiringOutcome::Received)?).await?;
        accept_report(&mut first, &firing, 2, report_request(&firing, 2, FiringOutcome::Unknown)?).await?;
        let newer = fixture::automation(client, &id, None).await?;
        if (*required_value(&newer, "/firings/0/reportSeq")?) != 3 || (*required_value(&newer, "/firings/0/outcomeJson")?) != (*required_value(&first_report, "/firings/0/outcomeJson")?) || !(*required_value(&newer, "/firings/0/batchId")?).is_null() { return Err("carried report state regressed or acquired a Batch".into()); }
        Ok(())
    }.await;
    let first_closed = first.close().await;
    let second_closed = second.close().await;
    proof?;
    first_closed?;
    second_closed?;
    Ok(())
}

pub(super) fn wake_payload(destination: &str) -> ConformanceResult<Value> {
    let mut payload = schedule_payload(destination);
    let target: SessionRef = serde_json::from_value(
        json!({"endpoint":{"serviceId":destination,"endpointId":"codex-local"},"sessionId":"carried-wake-target"}),
    )?;
    let mut creator = target.clone();
    creator.session_id = "original-creator-session".to_owned().try_into()?;
    *required_value_mut(&mut payload, "/creator")? =
        serde_json::to_value(WakeCreator::Session(creator.clone()))?;
    *required_value_mut(&mut payload, "/definition")? = json!({"kind":"wake","value":{
        "configuration":{"target":target,"origin":FiringMessageOrigin::Agent { sender: creator },"delivery":"auto","generationGuard":null,"expiresAt":"2026-10-31T10:40:00Z","state":"active"},
        "timing":{"kind":"interval","seconds":600,"firstDue":"2026-10-31T10:05:00Z"}
    }});
    *required_value_mut(&mut payload, "/carriedFiring")? =
        json!({"firingId":durable_steps::Uuid::now_v7().to_string(),"dueMs":1793440800000_i64});
    Ok(payload)
}

pub(super) fn require_received(
    row: &Value,
    payload: &Value,
    importer: &str,
) -> ConformanceResult<()> {
    if (*required_value(row, "/firings")?).as_array().map(Vec::len) != Some(1)
        || (*required_value(row, "/firings/0/firingId")?)
            != (*required_value(payload, "/carriedFiring/firingId")?)
        || (*required_value(row, "/firings/0/dueMs")?)
            != (*required_value(payload, "/carriedFiring/dueMs")?)
        || (*required_value(row, "/firings/0/state")?) != "received"
        || (*required_value(row, "/firings/0/reportServiceId")?) != importer
        || !(*required_value(row, "/firings/0/batchId")?).is_null()
        || !(*required_value(row, "/firings/0/reportSeq")?).is_null()
        || (*required_value(row, "/batches")?).as_array().map(Vec::len) != Some(0)
    {
        eprintln!(
            "HANDOVER_CARRIED_CUSTODY_MISMATCH {}",
            json!({"actual":row,"payload":payload,"importer":importer})
        );
        return Err("carried Wake lost received/importer-owned/no-Batch custody".into());
    }
    Ok(())
}

pub(super) fn report_request(
    firing: &FiringId,
    sequence: u64,
    outcome: FiringOutcome,
) -> ConformanceResult<Value> {
    Ok(
        json!({"kind":"reportWorkItems","value":{"reports":[WorkItemReport::Firing(FiringReport { firing_id: firing.clone(), seq: sequence.try_into()?, outcome })]}}),
    )
}

pub(super) async fn accept_report(
    machine: &mut NativeBoardSession,
    firing: &FiringId,
    sequence: u64,
    request: Value,
) -> ConformanceResult<()> {
    let AutomationAnswer::Reports(acknowledgement) = raw_call(machine, request).await? else {
        return Err("normal carried FiringReport refused".into());
    };
    if acknowledgement.accepted.len() != 1 || !acknowledgement.accepted.first().is_some_and(|accepted| matches!(&accepted.subject, WorkReportSubject::Firing { firing_id } if firing_id == firing) && u64::from(accepted.seq) == sequence) { return Err("carried report acknowledgement lost subject/sequence".into()); }
    Ok(())
}
