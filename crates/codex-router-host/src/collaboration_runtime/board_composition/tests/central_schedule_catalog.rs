//! Public schedule creation must use the organization's instruction and definition owners.
use super::central_catalog_fixture::{
    CatalogHosts, ProofResult, connected_transport, future_wake_time,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, CatalogAutomation, ExecutionDestination,
    InstructionCreateParams, OperationId, ScheduleCreateRequest, ScheduleDefinition,
    ScheduleEnableRequest, ScheduleShowRequest, ScheduleUpdateRequest,
};
use collaboration_runtime::CentralAutomationTransport;

#[tokio::test]
async fn public_schedule_create_uses_central_instruction_without_local_definition_writes()
-> ProofResult {
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        drop(connected_transport(&hosts.first).await?);
        let transport = connected_transport(&hosts.second).await?;
        let first =
            CollaborationClient::connect(&hosts.first_root, "schedule-catalog-a", "1").await?;
        let instruction = first
            .create_instruction(InstructionCreateParams {
                operation_id: OperationId::generate(),
                text: "A centrally owned schedule instruction"
                    .to_owned()
                    .try_into()?,
            })
            .await?;
        let request = ScheduleCreateRequest {
            operation_id: OperationId::generate(),
            definition: ScheduleDefinition {
                instruction_id: instruction.instruction_id,
                timing: serde_json::from_value(
                    serde_json::json!({"kind":"interval","seconds":600}),
                )?,
                enabled: false,
                destination: ExecutionDestination::Unprepared,
                execution_timeout_seconds: None,
                model: Some("gpt-6-luna".to_owned()),
                effort: Some("low".to_owned()),
            },
        };
        // Act: disabled and unprepared, so this never starts native work or sends model input.
        let created = first
            .create_schedule(request.clone())
            .await
            .map_err(|error| {
                format!("public schedule create still depends on local definition truth: {error}")
            })?;
        let answer = transport
            .call(AutomationCall::ScheduleShow(ScheduleShowRequest {
                schedule_id: created.schedule_id.clone(),
            }))
            .await?;
        let AutomationAnswer::Schedule(central) = answer else {
            return Err("public schedule create never reached central".into());
        };
        let replay = first.create_schedule(request).await?;
        if serde_json::to_value(&created)? != central_schedule_json(serde_json::to_value(central)?)?
            || serde_json::to_value(&created)? != serde_json::to_value(replay)?
        {
            return Err("central schedule identity or revision changed on replay".into());
        }
        // Assert: ordinary catalog CRUD creates no local Schedule or Instruction definitions.
        use sqlx::Connection;
        for root in [&hosts.first_root, &hosts.second_root] {
            let mut sql = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new().filename(root.join("automation.sqlite")),
            )
            .await?;
            let instructions: i64 =
                sqlx::query_scalar("SELECT count(*) FROM instruction_documents")
                    .fetch_one(&mut sql)
                    .await?;
            let schedules: i64 = sqlx::query_scalar("SELECT count(*) FROM schedule_definitions")
                .fetch_one(&mut sql)
                .await?;
            sql.close().await?;
            if instructions != 0 || schedules != 0 {
                return Err("public schedule create retained a local definition owner".into());
            }
        }
        Ok(())
    }
    .await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn public_schedule_changes_keep_central_revision_refusals_and_mutation_evidence()
-> ProofResult {
    use collaboration_client::{CentralCatalogFailure, ScheduleClientError};
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        drop(connected_transport(&hosts.first).await?);
        let transport = connected_transport(&hosts.second).await?;
        let first = CollaborationClient::connect(&hosts.first_root, "schedule-change-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "schedule-change-b", "1").await?;
        let instruction = first.create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(), text: "Central schedule change proof".to_owned().try_into()?,
        }).await?;
        let created = first.create_schedule(ScheduleCreateRequest {
            operation_id: OperationId::generate(),
            definition: ScheduleDefinition {
                instruction_id: instruction.instruction_id,
                timing: serde_json::from_value(serde_json::json!({"kind":"interval","seconds":600}))?,
                enabled: false, destination: ExecutionDestination::Unprepared,
                execution_timeout_seconds: None, model: Some("gpt-6-luna".to_owned()), effort: Some("low".to_owned()),
            },
        }).await?;
        let update = ScheduleUpdateRequest {
            operation_id: OperationId::generate(), schedule_id: created.schedule_id.clone(),
            expected_change_id: created.change_id.clone(),
            definition: ScheduleDefinition {
                timing: serde_json::from_value(serde_json::json!({"kind":"interval","seconds":1200}))?,
                ..created.definition.clone()
            },
        };
        // Act: B changes A's schedule, preserving central's complete mutation evidence.
        let changed = central_mutation(second.update_schedule(update.clone()).await?)?;
        let central = transport.call(AutomationCall::ScheduleUpdate(update)).await?;
        if serde_json::to_value(&changed)? != central_mutation_json(central)? {
            return Err("public schedule update discarded central mutation evidence".into());
        }
        let collaboration_client::CatalogAutomationResult::Schedule(updated) = &changed.automation else {
            return Err("schedule update substituted a different catalog kind".into());
        };
        if updated.change_id == created.change_id || updated.schedule_id != created.schedule_id || !changed.already_sent.is_empty() {
            return Err("disabled schedule change lost revision or invented sent work".into());
        }
        let shown = first.read_schedule(ScheduleShowRequest { schedule_id: created.schedule_id.clone() }).await?;
        if serde_json::to_value(shown)? != serde_json::to_value(&changed.automation)? {
            return Err("cross-Host public schedule read lost central state".into());
        }
        let disable = ScheduleEnableRequest { operation_id: OperationId::generate(), schedule_id: created.schedule_id.clone() };
        let disabled = central_mutation(first.disable_schedule(disable.clone()).await?)?;
        let central = transport.call(AutomationCall::ScheduleDisable(disable)).await?;
        if serde_json::to_value(&disabled)? != central_mutation_json(central)? {
            return Err("schedule disable discarded actual alreadySent IDs".into());
        }
        let collaboration_client::CatalogAutomationResult::Schedule(disabled) = disabled.automation else { return Err("disabled schedule snapshot missing".into()); };
        let operation = OperationId::generate();
        let stale = second.update_schedule(ScheduleUpdateRequest {
            operation_id: operation.clone(), schedule_id: created.schedule_id.clone(),
            expected_change_id: created.change_id, definition: created.definition,
        }).await;
        if !matches!(stale, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused { operation_id: Some(actual), error, .. }))
            if actual == operation && matches!(&*error, AutomationRefusal::ScheduleChangeConflict { current_change_id } if current_change_id == &disabled.change_id)) {
            return Err("public stale schedule edit lost actual head or operation identity".into());
        }
        let operation = OperationId::generate();
        let unprepared = second.enable_schedule(ScheduleEnableRequest {
            operation_id: operation.clone(), schedule_id: created.schedule_id.clone(),
        }).await;
        if !matches!(unprepared, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused { operation_id: Some(actual), error, .. }))
            if actual == operation && matches!(&*error, AutomationRefusal::InvalidRequest { field, reason: collaboration_protocol::AutomationRequestConstraint::UnpreparedDestination } if field == "enabled")) {
            return Err("unprepared enable lost central's typed pre-effect refusal".into());
        }
        let shown = second.read_schedule(ScheduleShowRequest { schedule_id: created.schedule_id }).await?;
        if serde_json::to_value(shown)? != serde_json::to_value(collaboration_client::CatalogAutomationResult::Schedule(disabled))? {
            return Err("refused schedule mutations changed central state".into());
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn public_schedule_pages_cross_hosts_without_inventing_cursors_or_wake_rows() -> ProofResult {
    use collaboration_protocol::{
        AutomationPageRequest, MachineWorkProtocol, PullAutomationBatches,
    };
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        drop(connected_transport(&hosts.first).await?);
        let transport = connected_transport(&hosts.second).await?;
        let first = CollaborationClient::connect(&hosts.first_root, "schedule-list-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "schedule-list-b", "1").await?;
        // A real pull establishes the receiver capability causally, before creating a Wake.
        if !matches!(transport.call(AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None, max_bytes: 1_048_576, work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        })).await?, AutomationAnswer::Batches(_)) {
            return Err("catalog fixture capability was not admitted".into());
        }
        let wake = transport.call(AutomationCall::WakeSend(serde_json::from_value(serde_json::json!({
            "operationId":OperationId::generate(), "requestedBy":{"kind":"router"},"replace":false,
            "message":{"target":{"endpoint":{"serviceId":second.identity().service_id,"endpointId":"codex-local"},"sessionId":"inert-future-catalog-wake"},"content":{"kind":"router","text":"Future fixture; no native backend"},"delivery":"queue"},
            "timing":{"kind":"at","at":future_wake_time()?},"expiry":{"kind":"after","seconds":7200}
        }))?)).await?;
        if !matches!(&wake, AutomationAnswer::Wake(_)) {
            return Err(format!("real central mixed-list Wake was not created: {wake:?}").into());
        }
        let instruction = first.create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(), text: "Central schedule list proof".to_owned().try_into()?,
        }).await?;
        let mut expected = std::collections::BTreeSet::new();
        for _ in 0..2 {
            let schedule = first.create_schedule(ScheduleCreateRequest {
                operation_id: OperationId::generate(),
                definition: ScheduleDefinition {
                    instruction_id: instruction.instruction_id.clone(),
                    timing: serde_json::from_value(serde_json::json!({"kind":"interval","seconds":600}))?,
                    enabled: false, destination: ExecutionDestination::Unprepared,
                    execution_timeout_seconds: None, model: Some("gpt-6-luna".to_owned()), effort: Some("low".to_owned()),
                },
            }).await?;
            expected.insert(schedule.schedule_id.as_str().to_owned());
        }
        let mut request = AutomationPageRequest { cursor: None, limit: 1.try_into()? };
        let mut first_cursor = None;
        let mut second_cursor = None;
        let mut observed = std::collections::BTreeSet::new();
        let mut empty_pages = 0;
        for ordinal in 0..3 {
            let AutomationAnswer::List { automations, next } = transport.call(AutomationCall::List(request.clone())).await? else {
                return Err("central organization list missing".into());
            };
            let mut expected_records = Vec::new();
            for automation in automations {
                if let CatalogAutomation::Schedule(schedule) = automation {
                    expected_records.push(serde_json::json!({"kind":"schedule","value":central_schedule_json(serde_json::to_value(schedule)?)?}));
                }
            }
            let page = first.list_schedules(AutomationPageRequest { cursor:first_cursor.clone(),..request.clone() }).await?;
            let other = second.list_schedules(AutomationPageRequest { cursor:second_cursor.clone(),..request.clone() }).await?;
            if serde_json::to_value(&page.records)? != serde_json::to_value(&expected_records)?
                || serde_json::to_value(&other.records)? != serde_json::to_value(&expected_records)?
                || page.next_cursor.is_some() != next.is_some() || other.next_cursor.is_some() != next.is_some() {
                return Err("public Schedule phases changed central rows or continuation boundaries".into());
            }
            if ordinal == 0 {
                let foreign = second.list_schedules(AutomationPageRequest {cursor:page.next_cursor.clone(),..request.clone()}).await;
                if !matches!(foreign,Err(collaboration_client::AutomationInspectionClientError::Rejected(error))
                    if matches!(error.kind, collaboration_protocol::AutomationInspectionFailureKind::InvalidField)
                        && error.field.as_deref() == Some("cursor")) {
                    return Err("Schedule listing accepted another Host's service cursor".into());
                }
            }
            first_cursor = page.next_cursor;
            second_cursor = other.next_cursor;
            if page.records.is_empty() { empty_pages += 1; }
            for record in page.records {
                let value = serde_json::to_value(record)?;
                let id = value.pointer("/value/scheduleId").and_then(serde_json::Value::as_str).ok_or("schedule page included a Wake or lost its catalog tag")?;
                observed.insert(id.to_owned());
            }
            match (ordinal, next) {
                (0 | 1, Some(cursor)) => request.cursor = Some(cursor),
                (2, None) => {},
                _ => return Err("central mixed list ended early or repeated a page".into()),
            }
        }
        if observed != expected || empty_pages != 1 {
            return Err("schedule-specific listing lost a definition or leaked the Wake row".into());
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

fn central_mutation(
    value: collaboration_client::ScheduleMutationResponse,
) -> ProofResult<collaboration_client::CatalogMutationResult> {
    match value {
        collaboration_client::ScheduleMutationResponse::Central(value) => Ok(value),
        collaboration_client::ScheduleMutationResponse::Local(_) => {
            Err("new central Schedule control changed to Local".into())
        }
    }
}
fn central_schedule_json(mut value: serde_json::Value) -> ProofResult<serde_json::Value> {
    let object = value
        .as_object_mut()
        .ok_or("central Schedule must be an object")?;
    if object.remove("activeRunId").is_none() || object.remove("waitingRunId").is_none() {
        return Err("shared central Schedule lost its source occupancy placeholders".into());
    }
    Ok(value)
}
fn central_mutation_json(answer: AutomationAnswer) -> ProofResult<serde_json::Value> {
    let AutomationAnswer::Mutation {
        automation: CatalogAutomation::Schedule(value),
        already_sent,
    } = answer
    else {
        return Err("actual central Schedule mutation missing".into());
    };
    Ok(
        serde_json::json!({"automation":{"kind":"schedule","value":central_schedule_json(serde_json::to_value(value)?)?},"alreadySent":already_sent}),
    )
}
