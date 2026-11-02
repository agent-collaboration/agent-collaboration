//! Central parents do not require local definition projections before execution admission.
use collaboration_client::{AutomationInspectionClientError, CentralCatalogFailure};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, DeliveryListRequest,
    InstructionCreateParams, MachineWorkProtocol, OperationId, PullAutomationBatches,
    RunListRequest, ScheduleId, WakeupId,
};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
use fixture::{CatalogFixture, ProofResult, SERVICE};

fn require_missing<Value: std::fmt::Debug>(
    result: Result<Value, AutomationInspectionClientError>,
) -> ProofResult {
    if matches!(result, Err(AutomationInspectionClientError::Catalog(
        CentralCatalogFailure::Refused { error, .. }
    )) if matches!(*error,AutomationRefusal::NotFound))
    {
        Ok(())
    } else {
        Err("absent central parent did not retain typed NotFound".into())
    }
}
fn require_unavailable<Value: std::fmt::Debug>(
    result: Result<Value, AutomationInspectionClientError>,
) -> ProofResult {
    if matches!(
        result,
        Err(AutomationInspectionClientError::Catalog(
            CentralCatalogFailure::CentralUnreachable {
                operation_id: None,
                ..
            }
        ))
    ) {
        Ok(())
    } else {
        Err("unreachable central parent was hidden by an empty local page".into())
    }
}

#[tokio::test]
async fn central_schedule_without_local_projection_has_empty_runs_and_preserves_parent_errors()
-> ProofResult {
    let mut fixture = CatalogFixture::open(|transport| transport, true).await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("central-run-parent").await?;
        let instruction = client.create_instruction(InstructionCreateParams {
            operation_id:OperationId::generate(),text:"No native execution".to_owned().try_into()?,
        }).await?;
        let schedule = client.create_schedule(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},
                "enabled":false,"destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,
                "model":"gpt-6-luna","effort":"low"
            }
        }))?).await?;
        let request=RunListRequest {schedule_id:schedule.schedule_id,cursor:None,limit:50.try_into()?};
        let page=client.list_runs(request.clone()).await?;
        if !page.records.is_empty() || page.next_cursor.is_some() { return Err("central-only Schedule invented local runs or a cursor".into()); }
        require_missing(client.list_runs(RunListRequest {
            schedule_id:ScheduleId::try_from(uuid::Uuid::now_v7().to_string())?,..request.clone()
        }).await)?;
        fixture.runtime.disconnect().await?;
        require_unavailable(client.list_runs(request).await)?;
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn central_wake_without_local_projection_has_empty_deliveries_and_preserves_parent_errors()
-> ProofResult {
    let mut fixture = CatalogFixture::open_with_controls(|transport| transport,true,vec![serde_json::from_value(json!({
        "address":{"kind":"fleet"},"command":{"kind":"automationClock","value":{"kind":"start","now":"2030-01-01T00:00:00Z"}}
    }))?]).await?;
    let proof: ProofResult = async {
        let ready=fixture.transport.call(AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after:None,max_bytes:1_048_576,work_protocols:vec![MachineWorkProtocol::MachineWorkV1],
        })).await?;
        if !matches!(ready,AutomationAnswer::Batches(_)) {return Err("fixture receiving capability unavailable".into());}
        let client=fixture.api.client("central-delivery-parent").await?;
        let created=client.send_wakeup(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"requestedBy":{"kind":"owner"},"replace":false,
            "message":{"target":{"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"no-native-history-parent"},
                "content":{"kind":"humanUser","text":"not dispatched"},"delivery":"queue"},
            "timing":{"kind":"at","at":"2030-01-01T00:05:00Z"},"expiry":{"kind":"after","seconds":7200}
        }))?).await?;
        let collaboration_client::WakeCreateResponse::Central {value:wake}=created else {return Err("new Wake did not use central owner".into());};
        let request=DeliveryListRequest {wakeup_id:Some(wake.definition.wakeup_id),cursor:None,limit:50.try_into()?};
        let page=client.list_deliveries(request.clone()).await?;
        if !page.records.is_empty() || page.next_cursor.is_some() {return Err("central-only Wake invented local deliveries or a cursor".into());}
        require_missing(client.list_deliveries(DeliveryListRequest {
            wakeup_id:Some(WakeupId::try_from(uuid::Uuid::now_v7().to_string())?),..request.clone()
        }).await)?;
        fixture.runtime.disconnect().await?;
        require_unavailable(client.list_deliveries(request).await)?;
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
