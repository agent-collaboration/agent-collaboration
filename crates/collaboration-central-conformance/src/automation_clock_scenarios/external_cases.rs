//! M2 settlement intake over seeded import metadata; M5 handover is not claimed.
use super::{call, fixture, open_machine, pull, require_instruction};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::{AutomationFixtureMutation, RegistryTime};
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationCall, AutomationId, AutomationRefusal,
    ExecutionDestination, FetchContents, FiringWork, GridTime, InstructionCreateParams,
    InstructionUpdateParams, LocalRunOutcome, LocalRunSettled, ScheduleCreateRequest,
    ScheduleDefinition, ScheduleUpdateRequest, TimingRequest, WorkEntry,
};

pub(super) async fn run(client: &ConformanceClient) -> ConformanceResult<()> {
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut machine = open_machine(client, &service).await?;
    let proof = scenario(client, &mut machine, &service).await;
    let closed = machine.close().await;
    proof?;
    closed?;
    Ok(())
}
async fn scenario(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    service: &str,
) -> ConformanceResult<()> {
    assert!(pull(machine).await?.batches.is_empty());
    let initial = require_instruction(
        call(
            machine,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                text: "before pending edit".to_owned().try_into()?,
            }),
        )
        .await?,
    )?;
    let definition = ScheduleDefinition {
        instruction_id: initial.instruction_id.clone(),
        timing: TimingRequest::Interval {
            seconds: 300.try_into()?,
            first_due: None,
        },
        enabled: true,
        destination: ExecutionDestination::OwnedThread {
            target: serde_json::from_value(serde_json::json!({
                "endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"external-target"
            }))?,
            cwd: "/workspace".into(),
        },
        execution_timeout_seconds: None,
        model: None,
        effort: Some("high".into()),
    };
    let created = call(
        machine,
        AutomationCall::ScheduleCreate(ScheduleCreateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            definition: definition.clone(),
        }),
    )
    .await?;
    let AutomationAnswer::Schedule(schedule) = created else {
        return Err("external fixture Schedule refused".into());
    };
    let id: AutomationId = schedule.schedule_id.as_str().to_owned().try_into()?;
    let run_id: collaboration_protocol::RunId =
        durable_steps::Uuid::now_v7().to_string().try_into()?;
    fixture::mutate(
        client,
        &id,
        AutomationFixtureMutation::ExternalOccupant {
            service_id: service.to_owned().try_into()?,
            import_id: durable_steps::Uuid::now_v7(),
            run_id: run_id.clone(),
        },
    )
    .await?;
    let now = client
        .automation_clock(AutomationClockControl::Observe)
        .await?
        .now;
    let due = GridTime::next_after(now.0.timestamp_millis())?.unix_millis();
    advance(client, due).await?;
    advance(client, due + 600_000).await?;
    assert!(
        pull(machine).await?.batches.is_empty(),
        "external occupant let its successor cross cutoff"
    );
    let raw = fixture::automation(client, &id, None).await?;
    let pending = raw
        .get("firings")
        .and_then(serde_json::Value::as_array)
        .ok_or("external waiter missing")?;
    assert_eq!(
        pending.len(),
        1,
        "later dues did not fold into the one external waiter"
    );
    let pending = pending.first().ok_or("external waiter missing")?;
    assert_eq!(
        pending.get("dueMs").and_then(serde_json::Value::as_i64),
        Some(due)
    );
    assert_eq!(
        pending
            .get("foldedThroughMs")
            .and_then(serde_json::Value::as_i64),
        Some(due + 600_000)
    );
    assert!(
        pending
            .get("batchId")
            .is_some_and(serde_json::Value::is_null)
    );
    let waiting_id = pending
        .get("firingId")
        .and_then(serde_json::Value::as_str)
        .ok_or("waiter identity missing")?
        .to_owned();
    let updated = require_instruction(
        call(
            machine,
            AutomationCall::InstructionUpdate(InstructionUpdateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                instruction_id: initial.instruction_id,
                expected_revision_id: initial.revision_id.clone(),
                text: "after validated pending edit".to_owned().try_into()?,
            }),
        )
        .await?,
    )?;
    let mut changed = definition;
    changed.effort = Some("medium".into());
    let edited = call(
        machine,
        AutomationCall::ScheduleUpdate(ScheduleUpdateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            schedule_id: schedule.schedule_id,
            expected_change_id: schedule.change_id,
            definition: changed.clone(),
        }),
    )
    .await?;
    let AutomationAnswer::Mutation {
        automation: collaboration_protocol::CatalogAutomation::Schedule(edited),
        already_sent,
    } = edited
    else {
        return Err("pending external Schedule edit refused".into());
    };
    assert!(already_sent.is_empty());
    fixture::automation(client, &id, Some("{corrupt after pending edit".into())).await?;
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    let settle = LocalRunSettled {
        automation_id: id.clone(),
        run_id,
        outcome: LocalRunOutcome::Finished,
    };
    let mut wrong = settle.clone();
    wrong.run_id = durable_steps::Uuid::now_v7().to_string().try_into()?;
    assert!(matches!(
        call(machine, AutomationCall::LocalRunSettled(wrong)).await?,
        AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
    ));
    let foreign_service = durable_steps::Uuid::now_v7().to_string();
    let mut foreign = open_machine(client, &foreign_service).await?;
    let denied = call(
        &mut foreign,
        AutomationCall::LocalRunSettled(settle.clone()),
    )
    .await;
    let closed = foreign.close().await;
    assert!(matches!(
        denied?,
        AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
    ));
    closed?;
    assert!(matches!(
        call(machine, AutomationCall::LocalRunSettled(settle.clone())).await?,
        AutomationAnswer::Acknowledged
    ));
    let custody = pull(machine).await?;
    assert_eq!(
        custody.batches.len(),
        1,
        "external settlement did not freeze its waiter promptly"
    );
    let frozen = custody.batches.first().ok_or("released Batch missing")?;
    let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
    assert_eq!(batch.entries.entries().len(), 1);
    let Some(WorkEntry::Firing(entry)) = batch.entries.entries().first() else {
        return Err("external released Firing missing".into());
    };
    assert_eq!(entry.firing_id.as_str(), waiting_id);
    assert_eq!(entry.due_at.unix_millis(), due);
    assert_eq!(entry.revision, edited.change_id);
    assert_eq!(entry.content.revision_id, updated.revision_id);
    assert_ne!(entry.content.revision_id, initial.revision_id);
    let FiringWork::Schedule {
        definition: frozen_definition,
    } = &entry.work
    else {
        return Err("external work kind changed".into());
    };
    assert_eq!(
        serde_json::to_value(frozen_definition)?,
        serde_json::to_value(changed)?
    );
    let contents = call(
        machine,
        AutomationCall::FetchContents(FetchContents {
            contents: vec![entry.content.clone()],
        }),
    )
    .await?;
    assert!(
        matches!(contents,AutomationAnswer::Contents(ref records) if records.len()==1 && records.first().is_some_and(|record| record.text.as_str()=="after validated pending edit"))
    );
    assert!(matches!(
        call(machine, AutomationCall::LocalRunSettled(settle)).await?,
        AutomationAnswer::Acknowledged
    ));
    assert_eq!(
        serde_json::to_value(pull(machine).await?)?,
        serde_json::to_value(&custody)?,
        "duplicate settlement changed custody"
    );
    let raw = fixture::automation(client, &id, None).await?;
    assert!(
        raw.get("externalRunId")
            .is_some_and(serde_json::Value::is_null)
    );
    assert!(
        raw.get("faultJson")
            .is_some_and(serde_json::Value::is_string)
    );
    let pending = raw
        .get("firings")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first())
        .ok_or("released Firing row missing")?;
    assert!(
        pending
            .get("pendingEntryJson")
            .is_some_and(serde_json::Value::is_null)
    );
    println!(
        "AUTOMATION_EXTERNAL_SETTLEMENT_OBSERVATION {}",
        serde_json::json!({
            "automationId":id,"waitingFiringId":entry.firing_id,"dueMs":due,"frozenBatchId":frozen.batch_id,
            "source":"debug-seeded import metadata; actual central LocalRunSettled; M5 handover unverified",
            "pendingEditCaptured":true,"corruptBodyRetained":true,"foreignAndWrongRunRefused":true,
        })
    );
    Ok(())
}
async fn advance(client: &ConformanceClient, milliseconds: i64) -> ConformanceResult<()> {
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(milliseconds)
                    .ok_or("external due invalid")?,
            ),
        })
        .await?;
    Ok(())
}
