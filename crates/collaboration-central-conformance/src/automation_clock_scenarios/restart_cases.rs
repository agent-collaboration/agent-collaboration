//! A platform restart sits between durable creation and the first due callback.
use super::{call, fixture, open_machine, pull, require_instruction};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult, enroll_admitted_peer,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationCall, BatchAcknowledgement,
    ExecutionDestination, FiringOutcome, FiringReport, GridTime, InstructionCreateParams,
    ReportWorkItems, ScheduleCreateRequest, ScheduleDefinition, ScheduleId, TimingRequest,
    WorkEntry, WorkItemReport,
};

/// Driver-owned identities cross the process restart; executable work stays in Fleet.
pub struct AutomationRestartCase {
    service: String,
    peer: String,
    definition: ScheduleDefinition,
    early: (ScheduleId, i64),
    late: (ScheduleId, i64),
    now: RegistryTime,
}

pub async fn prepare_automation_restart(
    client: &ConformanceClient,
) -> ConformanceResult<AutomationRestartCase> {
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut machine = open_machine(client, &service).await?;
    let proof = prepare(client, &mut machine, service).await;
    let closed = machine.close().await;
    let prepared = proof?;
    closed?;
    Ok(prepared)
}

async fn prepare(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    service: String,
) -> ConformanceResult<AutomationRestartCase> {
    let before = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    assert!(
        before.next_alarm_ms.is_none(),
        "restart case needs an empty journal"
    );
    assert!(pull(machine).await?.batches.is_empty());
    let instruction = require_instruction(
        call(
            machine,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                text: "restart retains executable custody".to_owned().try_into()?,
            }),
        )
        .await?,
    )?;
    let early_ms = GridTime::next_after(before.now.0.timestamp_millis())?.unix_millis();
    let late_ms = early_ms
        .checked_add(600_000)
        .ok_or("restart due overflow")?;
    let definition = ScheduleDefinition {
        instruction_id: instruction.instruction_id,
        timing: at(late_ms)?,
        enabled: true,
        destination: ExecutionDestination::OwnedThread {
            target: serde_json::from_value(serde_json::json!({
                "endpoint":{"serviceId":service,"endpointId":"codex-local"},
                "sessionId":"restart-target"
            }))?,
            cwd: "/workspace".into(),
        },
        execution_timeout_seconds: None,
        model: None,
        effort: Some("high".into()),
    };
    let late = create(machine, definition.clone()).await?;
    let armed = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    assert_eq!(armed.next_alarm_ms, Some(late_ms));
    let mut earlier = definition.clone();
    earlier.timing = at(early_ms)?;
    let early = create(machine, earlier).await?;
    let advanced = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    assert_eq!(
        advanced.next_alarm_ms,
        Some(early_ms),
        "earlier create left the later alarm armed"
    );
    assert_eq!(
        advanced.callbacks.len(),
        armed.callbacks.len(),
        "future creation invoked a callback"
    );
    println!(
        "AUTOMATION_RESTART_PREPARED {}",
        serde_json::json!({
            "earlyId":early,"earlyDueMs":early_ms,"lateId":late,"lateDueMs":late_ms,
            "clock":advanced,
        })
    );
    Ok(AutomationRestartCase {
        service,
        peer: machine.0.peer.to_string(),
        definition,
        early: (early, early_ms),
        late: (late, late_ms),
        now: before.now,
    })
}

pub async fn resume_automation_restart(
    client: &ConformanceClient,
    prepared: AutomationRestartCase,
) -> ConformanceResult<()> {
    // This must precede enrollment, list, pull, and every other business call.
    let reopened = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    assert_eq!(reopened.now, prepared.now);
    assert_eq!(
        reopened.platform_alarm_before_control_ms,
        Some(prepared.early.1),
        "restart lost the armed deadline before the first passive observation"
    );
    assert_eq!(reopened.next_alarm_ms, Some(prepared.early.1));
    let fired = advance(client, prepared.early.1).await?;
    assert!(
        fired.callbacks.len() > reopened.callbacks.len(),
        "retained deadline did not invoke its callback"
    );
    let mut machine = NativeBoardSession(
        enroll_admitted_peer(
            client,
            &prepared.service,
            &prepared.peer,
            &format!("automation-catalog-{}", prepared.service),
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?,
    );
    let proof = after_restart(client, &mut machine, &prepared).await;
    let closed = machine.close().await;
    proof?;
    closed?;
    println!(
        "AUTOMATION_RESTART_RESULT {}",
        serde_json::json!({
            "firstControl":reopened,"firstDueCallback":fired,
            "businessReconnectAfterCallback":true,"drainedThenSecondCreate":true,
        })
    );
    Ok(())
}

async fn after_restart(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    prepared: &AutomationRestartCase,
) -> ConformanceResult<()> {
    consume(machine, &prepared.early).await?;
    let armed = drain_notice_turn(client, &prepared.early.0, Some(prepared.late.1)).await?;
    let quiet = advance(client, prepared.late.1 - 1).await?;
    assert_eq!(
        quiet.callbacks.len(),
        armed.callbacks.len(),
        "clock polled between deadlines"
    );
    advance(client, prepared.late.1).await?;
    consume(machine, &prepared.late).await?;
    let drained = drain_notice_turn(client, &prepared.early.0, None).await?;
    assert!(
        drained.next_alarm_ms.is_none(),
        "completed one-shots left a polling timer"
    );
    let second_due = GridTime::next_after(prepared.late.1)?.unix_millis();
    let mut definition = prepared.definition.clone();
    definition.timing = at(second_due)?;
    let second = create(machine, definition).await?;
    let rearmed = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    assert_eq!(
        rearmed.next_alarm_ms,
        Some(second_due),
        "creation after drain did not arm the clock"
    );
    let fired = advance(client, second_due).await?;
    assert!(fired.callbacks.len() > rearmed.callbacks.len());
    consume(machine, &(second, second_due)).await?;
    drain_notice_turn(client, &prepared.early.0, None).await?;
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    assert!(
        pull(machine).await?.batches.is_empty(),
        "duplicate alarm repeated a completed one-shot"
    );
    assert!(
        client
            .automation_clock(AutomationClockControl::Observe)
            .await?
            .next_alarm_ms
            .is_none()
    );
    Ok(())
}

pub(super) async fn drain_notice_turn(
    client: &ConformanceClient,
    schedule: &ScheduleId,
    expected: Option<i64>,
) -> ConformanceResult<crate::AutomationClockObservation> {
    let id = schedule.as_str().to_owned().try_into()?;
    for _ in 0..32 {
        let raw = fixture::automation(client, &id, None).await?;
        let journal = raw
            .get("journalEarliestMs")
            .ok_or("journal observation missing")?;
        let observed = client
            .automation_clock(AutomationClockControl::Observe)
            .await?;
        if observed.next_alarm_ms == expected {
            assert!(journal.is_null(), "notice work remained after the drain");
            return Ok(observed);
        }
        let next = observed.next_alarm_ms.ok_or("notice work lost its timer")?;
        let next_turn = observed
            .now
            .0
            .timestamp_millis()
            .checked_add(1)
            .ok_or("notice turn overflow")?;
        if let Some(owed) = journal.as_i64() {
            // Advance only a real scheduled journal continuation, including its
            // ordinary RPC retry, never a manufactured time/polling budget.
            assert!(
                next <= owed.max(next_turn),
                "timer was later than durable notice debt"
            );
        } else {
            // A min-only timer may retain one stale immediate turn after its
            // notice finishes. Any repeating business polling fails this check.
            assert_eq!(next, next_turn, "timer without business or journal debt");
        }
        let drained = advance(client, next).await?;
        assert_eq!(drained.callbacks.len(), observed.callbacks.len() + 1);
        println!(
            "AUTOMATION_NOTICE_TURN {}",
            serde_json::json!({"journalEarliestMs":journal,"callback":drained})
        );
    }
    Err("notice journal did not drain within its bounded operation steps".into())
}

async fn consume(
    machine: &mut NativeBoardSession,
    expected: &(ScheduleId, i64),
) -> ConformanceResult<()> {
    let custody = pull(machine).await?;
    assert_eq!(custody.batches.len(), 1);
    let frozen = custody.batches.first().ok_or("restart Batch missing")?;
    let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
    assert_eq!(batch.entries.entries().len(), 1);
    let Some(WorkEntry::Firing(entry)) = batch.entries.entries().first() else {
        return Err("restart Firing missing".into());
    };
    assert_eq!(entry.automation_id.as_str(), expected.0.as_str());
    assert_eq!(entry.due_at.unix_millis(), expected.1);
    assert!(matches!(
        call(
            machine,
            AutomationCall::AckAutomationBatches(AckAutomationBatches {
                batches: vec![BatchAcknowledgement {
                    batch_id: frozen.batch_id.clone(),
                    digest: frozen.digest.clone()
                }],
            })
        )
        .await?,
        AutomationAnswer::Acknowledged
    ));
    let reported = call(
        machine,
        AutomationCall::ReportWorkItems(ReportWorkItems::new(vec![WorkItemReport::Firing(
            FiringReport {
                firing_id: entry.firing_id.clone(),
                seq: 1.try_into()?,
                outcome: FiringOutcome::Failed {
                    reason: "fixture terminal settlement".to_owned().try_into()?,
                },
            },
        )])?),
    )
    .await?;
    assert!(
        matches!(reported, AutomationAnswer::Reports(ref accepted) if accepted.accepted.len()==1)
    );
    Ok(())
}
async fn create(
    machine: &mut NativeBoardSession,
    definition: ScheduleDefinition,
) -> ConformanceResult<ScheduleId> {
    match call(
        machine,
        AutomationCall::ScheduleCreate(ScheduleCreateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            definition,
        }),
    )
    .await?
    {
        AutomationAnswer::Schedule(schedule) => Ok(schedule.schedule_id),
        other => Err(format!("restart creation refused: {other:?}").into()),
    }
}
fn at(milliseconds: i64) -> ConformanceResult<TimingRequest> {
    Ok(TimingRequest::At {
        at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(milliseconds)
            .ok_or("restart due out of range")?
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            .try_into()?,
    })
}
async fn advance(
    client: &ConformanceClient,
    milliseconds: i64,
) -> ConformanceResult<crate::AutomationClockObservation> {
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(milliseconds)
                    .ok_or("restart clock out of range")?,
            ),
        })
        .await
}
