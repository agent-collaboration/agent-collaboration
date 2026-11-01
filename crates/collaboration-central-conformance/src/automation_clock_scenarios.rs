//! The same normal machine client observes durable due admission on both runtimes.
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::{call, open_machine, require_instruction, same_answer},
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationCall, ExecutionDestination, FiringWork,
    InstructionCreateParams, MachineWorkProtocol, PullAutomationBatches, ScheduleCreateRequest,
    ScheduleDefinition, TimingRequest, WorkEntry,
};
use sha2::{Digest, Sha256};
mod custody_cases;
mod expired_lease_cases;
mod expiry_cases;
mod external_cases;
mod failure_cases;
pub(crate) mod fixture;
mod large_batch_cases;
mod limit_cases;
mod restart_cases;
mod retention_cases;
mod wake_delivery_cases;
pub use restart_cases::{
    AutomationRestartCase, prepare_automation_restart, resume_automation_restart,
};

pub async fn run_automation_external_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    external_cases::run(client).await
}

pub async fn run_automation_limit_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    limit_cases::run(client).await
}

pub async fn run_automation_wake_delivery_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    wake_delivery_cases::run(client).await
}

pub async fn run_automation_expiry_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    expiry_cases::run(client).await
}

pub async fn run_automation_retention_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    retention_cases::run(client).await
}

pub async fn run_automation_expired_lease_conformance(
    client: &ConformanceClient,
    platform: AutomationClockPlatform,
) -> ConformanceResult<()> {
    expired_lease_cases::run(client, platform).await
}

#[derive(Clone, Copy)]
pub enum AutomationClockPlatform {
    Tokio,
    Workerd,
}

pub async fn run_automation_clock_conformance(
    client: &ConformanceClient,
    platform: AutomationClockPlatform,
) -> ConformanceResult<()> {
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut machine = open_machine(client, &service).await?;
    let proof = first_create_arms_durable_deadline(client, &mut machine, &service, platform).await;
    let closed = machine.close().await;
    proof?;
    closed?;
    Ok(())
}

async fn first_create_arms_durable_deadline(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    service: &str,
    platform: AutomationClockPlatform,
) -> ConformanceResult<()> {
    let before = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
    let due = RegistryTime("2026-10-31T10:05:00Z".parse()?);
    let instruction = require_instruction(
        call(
            machine,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                text: "Frozen instruction for the first central deadline"
                    .to_owned()
                    .try_into()?,
            }),
        )
        .await?,
    )?;
    assert!(
        pull(machine).await?.batches.is_empty(),
        "fresh machine already had custody"
    );
    let request = ScheduleCreateRequest {
        operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
        definition: ScheduleDefinition {
            instruction_id: instruction.instruction_id.clone(),
            timing: TimingRequest::Interval {
                seconds: 600.try_into()?,
                first_due: None,
            },
            enabled: true,
            destination: ExecutionDestination::OwnedThread {
                target: serde_json::from_value(serde_json::json!({
                    "endpoint":{"serviceId":service,"endpointId":"codex-local"},
                    "sessionId":"central-clock-target"
                }))?,
                cwd: "/workspace".into(),
            },
            execution_timeout_seconds: None,
            model: None,
            effort: Some("high".into()),
        },
    };
    let original = call(machine, AutomationCall::ScheduleCreate(request.clone())).await?;
    let AutomationAnswer::Schedule(schedule) = &original else {
        return Err(format!("first clock schedule refused: {original:?}").into());
    };
    assert_eq!(
        schedule
            .next_due_at
            .as_ref()
            .map(|at| String::from(at.clone())),
        Some("2026-10-31T10:05:00Z".into())
    );

    let armed = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    assert_eq!(armed.now, before);
    assert_eq!(
        armed.next_alarm_ms,
        Some(due.0.timestamp_millis()),
        "empty-journal create did not arm its saved first deadline"
    );
    let quiet = client
        .automation_clock(AutomationClockControl::Advance { now: before })
        .await?;
    assert_eq!(
        quiet.callbacks.len(),
        armed.callbacks.len(),
        "callback ran before the stored deadline"
    );
    assert!(
        pull(machine).await?.batches.is_empty(),
        "Firing admitted before its saved deadline"
    );
    let early = client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    assert_eq!(early.callbacks.len(), quiet.callbacks.len() + 1);
    assert_eq!(early.next_alarm_ms, Some(due.0.timestamp_millis()));
    assert!(
        pull(machine).await?.batches.is_empty(),
        "an early actual callback admitted a future Firing"
    );

    let fired = client
        .automation_clock(AutomationClockControl::Advance { now: due })
        .await?;
    assert!(
        fired.callbacks.len() > early.callbacks.len(),
        "due callback was not observed"
    );
    let automation_id = schedule.schedule_id.as_str().to_owned().try_into()?;
    let admitted = fixture::automation(client, &automation_id, None).await?;
    let first_identity = one_admission(&admitted, due.0.timestamp_millis())?;
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    let duplicated = fixture::automation(client, &automation_id, None).await?;
    assert_eq!(
        one_admission(&duplicated, due.0.timestamp_millis())?,
        first_identity,
        "duplicate callback changed the Firing identity"
    );
    restart_cases::drain_notice_turn(
        client,
        &schedule.schedule_id,
        Some(due.0.timestamp_millis() + 600_000),
    )
    .await?;
    let retried = fixture::automation(client, &automation_id, None).await?;
    assert_eq!(
        one_admission(&retried, due.0.timestamp_millis())?,
        first_identity,
        "shared notice retry changed the Firing identity"
    );
    println!(
        "AUTOMATION_ADMISSION_IDENTITY {}",
        serde_json::json!({
            "automationId":automation_id,"dueMs":due.0.timestamp_millis(),"firingId":first_identity,
            "countAfterInitial":1,"countAfterDuplicateCallback":1,"countAfterSharedNoticeRetry":1,
        })
    );
    let custody = pull(machine).await?;
    assert_eq!(
        custody.batches.len(),
        1,
        "first due schedule did not freeze one Batch"
    );
    let frozen = custody.batches.first().ok_or("first Batch missing")?;
    let stored = fixture::automation(
        client,
        &schedule.schedule_id.as_str().to_owned().try_into()?,
        None,
    )
    .await?;
    let saved_batch = stored
        .get("batches")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("batchId").and_then(serde_json::Value::as_str)
                    == Some(frozen.batch_id.as_str())
            })
        })
        .ok_or("actual frozen Batch row missing")?;
    assert_eq!(
        saved_batch
            .get("frozenBytes")
            .and_then(serde_json::Value::as_str)
            .map(str::as_bytes),
        Some(frozen.frozen_bytes.as_slice())
    );
    assert_eq!(
        saved_batch
            .get("digest")
            .and_then(serde_json::Value::as_str),
        Some(frozen.digest.as_str())
    );
    let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
    assert_eq!(batch.batch_id, frozen.batch_id);
    assert_eq!(String::from(batch.service_id), service);
    assert_eq!(batch.entries.entries().len(), 1);
    let Some(WorkEntry::Firing(entry)) = batch.entries.entries().first() else {
        return Err("first Batch did not contain a Firing".into());
    };
    assert_eq!(entry.automation_id.as_str(), schedule.schedule_id.as_str());
    assert_eq!(entry.due_at.unix_millis(), due.0.timestamp_millis());
    assert_eq!(entry.revision, schedule.change_id);
    assert_eq!(entry.content.revision_id, instruction.revision_id);
    let FiringWork::Schedule { definition } = &entry.work else {
        return Err("Schedule Firing changed work kind".into());
    };
    assert_eq!(
        serde_json::to_value(definition)?,
        serde_json::to_value(&request.definition)?
    );
    same_answer(
        call(machine, AutomationCall::ScheduleCreate(request)).await?,
        original.clone(),
    )?;

    // No acknowledgement: custody must return the exact committed ID and bytes.
    let resent = pull(machine).await?;
    assert_eq!(
        serde_json::to_value(&resent)?,
        serde_json::to_value(&custody)?
    );
    custody_cases::run(client, machine, schedule, entry, frozen).await?;
    large_batch_cases::run(client, machine, &schedule.definition).await?;
    failure_cases::run(client, machine, &schedule.definition, platform).await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The exact admission-count assertion is the conformance oracle; missing fixture fields propagate as errors."
)]
fn one_admission(raw: &serde_json::Value, due_ms: i64) -> ConformanceResult<String> {
    let firings = raw
        .get("firings")
        .and_then(serde_json::Value::as_array)
        .ok_or("raw admissions missing")?;
    let same_pair = firings
        .iter()
        .filter(|row| row.get("dueMs").and_then(serde_json::Value::as_i64) == Some(due_ms))
        .collect::<Vec<_>>();
    println!(
        "AUTOMATION_RAW_ADMISSION_PAIR {}",
        serde_json::json!({"dueMs":due_ms,"firings":same_pair})
    );
    assert_eq!(
        same_pair.len(),
        1,
        "one admission per (automation,due) violated"
    );
    Ok(same_pair
        .first()
        .ok_or("admission row missing after exact count")?
        .get("firingId")
        .and_then(serde_json::Value::as_str)
        .ok_or("admission identity missing")?
        .to_owned())
}

async fn pull(
    machine: &mut NativeBoardSession,
) -> ConformanceResult<collaboration_protocol::AutomationBatches> {
    match call(
        machine,
        AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None,
            max_bytes: 1_048_576,
            work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        }),
    )
    .await?
    {
        AutomationAnswer::Batches(batches) => {
            for frozen in &batches.batches {
                assert_eq!(
                    format!("{:x}", Sha256::digest(&frozen.frozen_bytes)),
                    frozen.digest.as_str(),
                    "custody bytes differ from their committed digest"
                );
            }
            Ok(batches)
        }
        other => Err(format!("clock custody pull failed: {other:?}").into()),
    }
}
