//! Public central Schedule snapshots preserve catalog fields without claiming local Run occupancy.
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, InstructionCreateParams, OperationId, ScheduleShowRequest,
};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
use fixture::{CatalogFixture, ProofResult};

fn assert_central_view(
    actual: &serde_json::Value,
    shared: &collaboration_protocol::ScheduleSnapshot,
) -> ProofResult {
    let mut expected = serde_json::to_value(shared)?;
    let object = expected
        .as_object_mut()
        .ok_or("shared Schedule must be an object")?;
    object.remove("activeRunId");
    object.remove("waitingRunId");
    if actual != &expected {
        return Err(format!(
            "central public Schedule claimed local occupancy or changed catalog fields: {actual}"
        )
        .into());
    }
    Ok(())
}

#[tokio::test]
async fn central_schedule_create_show_list_and_mutation_omit_only_local_occupancy() -> ProofResult {
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let proof: ProofResult=async {
        let client=fixture.api.client("central-schedule-shape").await?;
        let instruction=client.create_instruction(InstructionCreateParams {
            operation_id:OperationId::generate(),text:"No native work".to_owned().try_into()?,
        }).await?;
        let created=client.create_schedule(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},
                "enabled":false,"destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,
                "model":"gpt-6-luna","effort":"low"
            }
        }))?).await?;
        let show=ScheduleShowRequest {schedule_id:created.schedule_id.clone()};
        let AutomationAnswer::Schedule(central)=fixture.transport.call(AutomationCall::ScheduleShow(show.clone())).await? else {return Err("actual central Schedule missing".into());};
        assert_central_view(&serde_json::to_value(&created)?,&central)?;
        let shown=serde_json::to_value(client.read_schedule(show).await?)?;
        if shown["kind"]!="schedule" {return Err("central Schedule read lost its central tag".into());}
        assert_central_view(&shown["value"],&central)?;
        let listed=client.list_schedules(serde_json::from_value(json!({"cursor":null,"limit":50}))?).await?;
        let listed=serde_json::to_value(&listed)?;
        assert_central_view(&listed["records"][0]["value"],&central)?;
        let changed=client.disable_schedule(collaboration_protocol::ScheduleEnableRequest {
            operation_id:OperationId::generate(),schedule_id:created.schedule_id.clone(),
        }).await?;
        let AutomationAnswer::Schedule(current)=fixture.transport.call(AutomationCall::ScheduleShow(ScheduleShowRequest {schedule_id:created.schedule_id})).await? else {return Err("changed central Schedule missing".into());};
        let changed=serde_json::to_value(changed)?;
        assert_central_view(&changed["value"]["automation"]["value"],&current)?;
        if changed["value"]["alreadySent"]!=json!([]) {return Err("disabled shape fixture invented already-sent work".into());}
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
