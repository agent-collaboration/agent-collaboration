//! Export must reserve import-envelope space, not just fit its own response.
use collaboration_protocol::{AutomationAnswer, AutomationCall, OperationId, ScheduleShowRequest};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;

#[tokio::test]
async fn export_rejects_package_that_fits_response_but_not_import() -> fixture::ProofResult {
    let fixture = fixture::CatalogFixture::open(|transport| transport, true).await?;
    let proof: fixture::ProofResult = async {
        let client = fixture.api.client("roundtrip-boundary").await?;
        let instruction = client.create_instruction(collaboration_protocol::InstructionCreateParams {
            operation_id: OperationId::generate(), text: "x".to_owned().try_into()?,
        }).await?;
        let schedule = client.create_schedule(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},
                "enabled":false,"destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,
                "model":"gpt-6-luna","effort":"low"
            }
        }))?).await?;
        let request = ScheduleShowRequest { schedule_id: schedule.schedule_id.clone() };
        // Ownership selection needs its store, but neither definition exists locally.
        {
            let mut store = fixture.store.as_ref().ok_or("ownership store missing")?.lock().await;
            if !matches!(store.inspect_schedule::<collaboration_protocol::SessionRef,collaboration_protocol::EndpointRef>(&schedule.schedule_id).await,Err(automation_storage::StorageError::ScheduleNotFound))
                || !matches!(store.read_instruction(&instruction.instruction_id).await,Err(automation_storage::StorageError::InstructionNotFound)) {
                return Err("central export boundary fixture has a Local definition shadow".into());
            }
        }
        let encoded = client.export_schedule(request.clone()).await?.package_utf8;
        let response_size = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"2","result":{"packageUtf8":encoded}}))?.len();
        let text_size = collaboration_protocol::MAX_CONTROL_FRAME_BYTES - response_size - 16 + 1;
        client.update_instruction(collaboration_protocol::InstructionUpdateParams {
            operation_id: OperationId::generate(), instruction_id: instruction.instruction_id,
            expected_revision_id: instruction.revision_id, text: "x".repeat(text_size).try_into()?,
        }).await?;
        // Act: the exported response fits, but its larger import request must be refused.
        let result = client.export_schedule(request.clone()).await;
        let rejected = matches!(result, Err(collaboration_client::ScheduleClientError::Rejected(error))
            if error.field.as_deref() == Some("packageUtf8"));
        if !rejected {
            return Err("export admitted a package without reserving its larger import envelope".into());
        }
        let AutomationAnswer::Schedule(after) = fixture.transport.call(AutomationCall::ScheduleShow(request)).await? else {
            return Err("frame refusal lost the central Schedule".into());
        };
        if after.change_id != schedule.change_id { return Err("frame refusal mutated the central Schedule".into()); }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
