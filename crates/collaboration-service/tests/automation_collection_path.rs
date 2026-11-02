//! Central catalog discovery and local execution history retain their own scopes.
use collaboration_protocol::{
    AutomationPageRequest, DeliveryListRequest, InstructionCreateParams, InstructionText,
    OperationId, RevisionListRequest, RunListRequest, ScheduleCreateRequest,
};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
use fixture::{CatalogFixture, ProofResult};

#[tokio::test]
async fn sdk_lists_central_catalog_and_retains_scoped_local_history() -> ProofResult {
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let served = &fixture.api;
    let store = fixture
        .store
        .as_ref()
        .ok_or("local history fixture missing")?;
    let client = served.client("collection-test").await?;
    let first_operation_id = OperationId::generate();
    let first = client
        .create_instruction(InstructionCreateParams {
            operation_id: first_operation_id.clone(),
            text: InstructionText::try_from("First task".to_owned())?,
        })
        .await?;
    client
        .create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(),
            text: InstructionText::try_from("Second task".to_owned())?,
        })
        .await?;
    let page = client
        .list_instructions(AutomationPageRequest {
            cursor: None,
            limit: 1.try_into()?,
        })
        .await?;
    if page.records.len() != 1 || page.records[0].instruction_id != first.instruction_id {
        return Err("instruction listing order incorrect".into());
    }
    let cursor = page.next_cursor.ok_or("missing next instruction cursor")?;
    let revisions = client
        .list_instruction_revisions(RevisionListRequest {
            instruction_id: first.instruction_id.clone(),
            cursor: None,
            limit: 50.try_into()?,
        })
        .await?;
    if revisions.records.len() != 1 || revisions.records[0].revision_id != first.revision_id {
        return Err("revision listing identity incorrect".into());
    }
    let request: ScheduleCreateRequest = serde_json::from_value(
        json!({"operationId":OperationId::generate(),"definition":{"instructionId":first.instruction_id,"timing":{"kind":"interval","seconds":600},"enabled":false,"destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"medium"}}),
    )?;
    let schedule = client.create_schedule(request).await?;
    let schedules = client
        .list_schedules(AutomationPageRequest {
            cursor: None,
            limit: 50.try_into()?,
        })
        .await?;
    let [
        collaboration_client::ScheduleReadResponse::Schedule {
            value: listed_schedule,
        },
    ] = schedules.records.as_slice()
    else {
        return Err("schedule list did not preserve its central Schedule record".into());
    };
    if listed_schedule.schedule_id != schedule.schedule_id {
        return Err("schedule listing identity incorrect".into());
    }
    let (local_operation, local_instruction, local_schedule) = {
        let mut store = store.lock().await;
        if !matches!(
            store.read_instruction(&first.instruction_id).await,
            Err(automation_storage::StorageError::InstructionNotFound)
        ) {
            return Err("central Instruction was shadowed in local history".into());
        }
        // Arrange genuine pre-upgrade local receipts; catalog writes create none of them.
        let operation = OperationId::generate();
        let instruction = store
            .create_instruction(
                &operation,
                &InstructionText::try_from("Retained local task".to_owned())?,
                chrono::Utc::now().timestamp_millis(),
            )
            .await?;
        store
            .create_instruction(
                &OperationId::generate(),
                &InstructionText::try_from("Other retained local task".to_owned())?,
                chrono::Utc::now().timestamp_millis(),
            )
            .await?;
        let schedule = store
            .create_schedule(&automation_storage::ScheduleCreate::<
                collaboration_protocol::SessionRef,
                collaboration_protocol::EndpointRef,
            > {
                operation_id: OperationId::generate(),
                definition: agent_automation::ScheduleDefinition {
                    instruction_id: instruction.instruction_id.clone(),
                    timing: agent_automation::TimingRule::Interval { seconds: 600 },
                    enabled: false,
                    destination: agent_automation::ExecutionDestination::Unprepared,
                    execution_timeout_seconds: None,
                    model: Some("gpt-6-luna".into()),
                    effort: Some("low".into()),
                },
                imported_continuity: agent_automation::ContinuityInput::None,
                now_ms: chrono::Utc::now().timestamp_millis(),
            })
            .await?;
        (operation, instruction, schedule)
    };
    let runs = client
        .list_runs(RunListRequest {
            schedule_id: local_schedule.schedule_id.clone(),
            cursor: None,
            limit: 50.try_into()?,
        })
        .await?;
    let deliveries = client
        .list_deliveries(DeliveryListRequest {
            wakeup_id: None,
            cursor: None,
            limit: 50.try_into()?,
        })
        .await?;
    if !runs.records.is_empty() || !deliveries.records.is_empty() {
        return Err("listing invented native or timed work".into());
    }
    let history = client
        .read_automation_events(collaboration_protocol::AutomationEventsRequest {
            after: None,
            limit: 50.try_into()?,
        })
        .await?;
    if history.records.len() != 3
        || !history.records.iter().any(|event| {
            matches!(
                event.details,
                collaboration_protocol::AutomationEventDetails::ScheduleEdit { .. }
            )
        })
    {
        return Err("event history omitted typed schedule definition evidence".into());
    }
    let continued = client
        .read_automation_events(collaboration_protocol::AutomationEventsRequest {
            after: Some(history.next_cursor),
            limit: 50.try_into()?,
        })
        .await?;
    if !continued.records.is_empty() {
        return Err("event cursor replayed consumed records".into());
    }
    let operation = client
        .read_operation(collaboration_protocol::OperationShowRequest {
            operation_id: local_operation,
        })
        .await?;
    if !matches!(
        operation.state,
        collaboration_protocol::OperationState::Succeeded {
            outcome: collaboration_protocol::OperationSuccess::Instruction {
                method: collaboration_protocol::InstructionOperation::Create,
                ..
            }
        }
    ) || operation.resource_id != local_instruction.instruction_id.as_str()
    {
        return Err("operation inspection lost method-specific success evidence".into());
    }
    // Obtain this service's actual Schedule phase envelope. Only its inner central
    // token is replaced by the real Instruction token to challenge central's scope.
    let local_page = client
        .list_schedules(AutomationPageRequest {
            cursor: None,
            limit: 1.try_into()?,
        })
        .await?;
    if !matches!(local_page.records.as_slice(), [collaboration_client::ScheduleReadResponse::Local {value}]
        if value.schedule_id == local_schedule.schedule_id)
    {
        return Err("actual Local Schedule phase was not preserved".into());
    }
    use base64::Engine as _;
    let service_cursor = local_page
        .next_cursor
        .ok_or("actual Schedule phase continuation missing")?;
    let mut phase: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(service_cursor)?,
    )?;
    if phase["position"]["kind"] != "central" || !phase["position"]["after"].is_null() {
        return Err("actual Schedule continuation did not enter central phase".into());
    }
    phase["position"]["after"] = json!(cursor);
    let foreign_cursor =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&phase)?);
    let rejected = client
        .list_schedules(AutomationPageRequest {
            cursor: Some(foreign_cursor),
            limit: 50.try_into()?,
        })
        .await;
    if !matches!(&rejected, Err(collaboration_client::AutomationInspectionClientError::Catalog(collaboration_client::CentralCatalogFailure::Refused { error, .. }))
        if matches!(error.as_ref(), collaboration_protocol::AutomationRefusal::InvalidRequest { field, reason:collaboration_protocol::AutomationRequestConstraint::InvalidCursor } if field == "cursor"))
    {
        return Err(format!("cross-collection cursor lacked typed rejection: {rejected:?}").into());
    }
    fixture.close().await
}
