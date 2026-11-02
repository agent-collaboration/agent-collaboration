//! Real central catalog and public SDK path; native effects are absent.
use collaboration_client::{CentralCatalogFailure, ScheduleClientError, ScheduleReadResponse};
use collaboration_protocol::{
    AutomationRefusal, AutomationRequestConstraint, InstructionCreateParams, InstructionText,
    OperationId, ScheduleCreateRequest, ScheduleShowRequest,
};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
use fixture::{CatalogFixture, ProofResult};

#[tokio::test]
async fn sdk_creates_disabled_schedule_without_native_backend() -> ProofResult {
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("schedule-test").await?;
        let instruction = client
            .create_instruction(InstructionCreateParams {
                operation_id: OperationId::generate(),
                text: InstructionText::try_from("Inspect build status".to_owned())?,
            })
            .await?;
        let request: ScheduleCreateRequest = serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,
                "timing":{"kind":"interval","seconds":600},"enabled":false,
                "destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,
                "model":"gpt-6-luna","effort":"low"
            }
        }))?;
        let first = client.create_schedule(request.clone()).await?;
        let replay = client.create_schedule(request).await?;
        if serde_json::to_value(&first)? != serde_json::to_value(replay)? {
            return Err("schedule creation replay changed result".into());
        }
        let ScheduleReadResponse::Schedule { value: read } = client
            .read_schedule(ScheduleShowRequest {
                schedule_id: first.schedule_id.clone(),
            })
            .await?
        else {
            return Err("new schedule unexpectedly faulted".into());
        };
        if read.definition.enabled {
            return Err("new disabled schedule invented running work".into());
        }
        require_no_firings(&client, &read.schedule_id).await?;
        let operation = OperationId::generate();
        let rejected = client
            .enable_schedule(collaboration_protocol::ScheduleEnableRequest {
                operation_id: operation.clone(),
                schedule_id: first.schedule_id.clone(),
            })
            .await;
        if !matches!(rejected, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused {
            operation_id:Some(actual),error,..
        })) if actual == operation && matches!(&*error, AutomationRefusal::InvalidRequest {
            field,reason:AutomationRequestConstraint::UnpreparedDestination
        } if field == "enabled"))
        {
            return Err("unprepared schedule enabled or failed without typed feedback".into());
        }
        let ScheduleReadResponse::Schedule { value: current } = client
            .read_schedule(ScheduleShowRequest {
                schedule_id: first.schedule_id,
            })
            .await?
        else {
            return Err("refused schedule unexpectedly faulted".into());
        };
        if serde_json::to_value(&current)? != serde_json::to_value(read)? {
            return Err("refused enable changed the disabled schedule".into());
        }
        Ok(())
    }
    .await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn sdk_rejects_mode_change_and_thread_preparation_with_actionable_feedback() -> ProofResult {
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("mode-fixture").await?;
        let ready = fixture.transport.call(collaboration_protocol::AutomationCall::PullAutomationBatches(
            collaboration_protocol::PullAutomationBatches {
                after: None, max_bytes: 1_048_576,
                work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
            },
        )).await?;
        if !matches!(ready, collaboration_protocol::AutomationAnswer::Batches(_)) {
            return Err("mode fixture did not advertise receiving capability".into());
        }
        let instruction = client.create_instruction(InstructionCreateParams {
            operation_id:OperationId::generate(),text:"Check".to_owned().try_into()?,
        }).await?;
        let endpoint=json!({"serviceId":fixture::SERVICE,"endpointId":"codex-local"});
        let created=client.create_schedule(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},
                "enabled":false,"destination":{"kind":"freshEachRun","endpoint":endpoint,"cwd":"/isolated-test"},
                "executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"low"
            }
        }))?).await?;
        let mut definition=created.definition.clone();
        definition.destination=collaboration_protocol::ExecutionDestination::Unprepared;
        let operation=OperationId::generate();
        let changed=client.update_schedule(collaboration_protocol::ScheduleUpdateRequest {
            operation_id:operation.clone(),schedule_id:created.schedule_id.clone(),
            expected_change_id:created.change_id.clone(),definition,
        }).await;
        if !matches!(changed, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused {
            operation_id:Some(actual),error,..
        })) if actual == operation && matches!(&*error, AutomationRefusal::InvalidRequest {
            field,reason:AutomationRequestConstraint::ExecutionModeFixed
        } if field == "destination")) {
            return Err("mode update lost central's typed destination refusal".into());
        }
        let preparation=client.prepare_schedule(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"scheduleId":created.schedule_id,
            "destination":{"kind":"fresh","endpoint":endpoint,"cwd":"/isolated-test"}
        }))?).await;
        let Err(ScheduleClientError::Rejected(error))=preparation else {
            return Err("mode preparation lacked typed rejection".into());
        };
        if !matches!(error.kind,collaboration_protocol::ScheduleFailureKind::InvalidField)
            || error.field.as_deref()!=Some("destination")
            || !error.message.contains("fixed at creation")
            || !matches!(error.next_action,collaboration_protocol::ScheduleNextAction::CorrectRequest)
        {
            return Err(format!("mode preparation omitted field, reason or recovery action: {error:?}").into());
        }
        let ScheduleReadResponse::Schedule { value: current }=client.read_schedule(ScheduleShowRequest {
            schedule_id:created.schedule_id,
        }).await? else { return Err("mode refusal unexpectedly faulted schedule".into()); };
        if current.change_id!=created.change_id {
            return Err("rejected mode request changed schedule or started work".into());
        }
        require_no_firings(&client, &current.schedule_id).await?;
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}

async fn require_no_firings(
    client: &collaboration_client::CollaborationClient,
    id: &collaboration_protocol::ScheduleId,
) -> ProofResult {
    let page = client
        .list_firings(collaboration_protocol::FiringListRequest {
            automation_id: id.as_str().to_owned().try_into()?,
            cursor: None,
            limit: 50.try_into()?,
        })
        .await?;
    if !page.records.is_empty() || page.next_cursor.is_some() {
        return Err("disabled/refused Schedule invented admitted central work".into());
    }
    Ok(())
}
