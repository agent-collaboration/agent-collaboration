//! Real retained custody and report ownership survive a calendar-window prune.
use super::{call, fixture, open_machine, pull, require_instruction};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult, enroll_admitted_peer,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationCall, AutomationPageRequest,
    AutomationRefusal, BatchAcknowledgement, ExecutionDestination, FiringEntry, FiringOutcome,
    FiringReport, FrozenAutomationBatch, GridTime, InstructionCreateParams, MachineWorkProtocol,
    PullAutomationBatches, ReportWorkItems, ScheduleCreateRequest, ScheduleDefinition,
    TimingRequest, WorkEntry, WorkItemReport,
};

pub(super) async fn run(client: &ConformanceClient) -> ConformanceResult<()> {
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut machine = open_machine(client, &service).await?;
    let peer = machine.0.peer;
    let prepared = prepare(client, &mut machine, &service).await;
    let closed = machine.close().await;
    let (running, unacked, now) = prepared?;
    closed?;
    let after_retention = now
        .0
        .checked_add_months(chrono::Months::new(2))
        .and_then(|time| time.checked_add_signed(chrono::Duration::days(4)))
        .ok_or("retention clock is out of range")?;
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(after_retention),
        })
        .await?;
    // Reconnect the same enrolled identity, with a new transport session only.
    let mut machine = NativeBoardSession(
        enroll_admitted_peer(
            client,
            &service,
            &peer.to_string(),
            &format!("automation-catalog-{service}"),
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?,
    );
    let foreign_service = durable_steps::Uuid::now_v7().to_string();
    let mut foreign = match open_machine(client, &foreign_service).await {
        Ok(machine) => machine,
        Err(error) => {
            machine.close().await?;
            return Err(error);
        }
    };
    let proof = after_prune(
        client,
        &mut machine,
        &mut foreign,
        &service,
        &running,
        &unacked,
    )
    .await;
    let closed = machine.close().await;
    let foreign_closed = foreign.close().await;
    proof?;
    closed?;
    foreign_closed?;
    println!(
        "AUTOMATION_RETENTION_OBSERVATION ACKed Batch pruned after two calendar months; ongoing owner report accepted; foreign report refused; unACKed bytes retained"
    );
    Ok(())
}

async fn prepare(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    service: &str,
) -> ConformanceResult<(FiringEntry, FrozenAutomationBatch, RegistryTime)> {
    let instruction = require_instruction(
        call(
            machine,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                text: "retention instruction".to_owned().try_into()?,
            }),
        )
        .await?,
    )?;
    let advertised = call(
        machine,
        AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None,
            max_bytes: 1_048_576,
            work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        }),
    )
    .await?;
    assert!(matches!(advertised, AutomationAnswer::Batches(_)));
    let now = client
        .automation_clock(AutomationClockControl::Observe)
        .await?
        .now;
    let first_due = GridTime::next_after(now.0.timestamp_millis())?.unix_millis();
    let definition = ScheduleDefinition {
        instruction_id: instruction.instruction_id,
        timing: at(first_due)?,
        enabled: true,
        destination: ExecutionDestination::OwnedThread {
            target: serde_json::from_value(serde_json::json!({
                "endpoint":{"serviceId":service,"endpointId":"codex-local"},
                "sessionId":"retention-target"
            }))?,
            cwd: "/workspace".into(),
        },
        execution_timeout_seconds: None,
        model: None,
        effort: Some("high".into()),
    };
    create(machine, definition.clone()).await?;
    advance(client, first_due).await?;
    let (running, first) = one_batch(machine).await?;
    assert!(matches!(
        call(
            machine,
            AutomationCall::AckAutomationBatches(AckAutomationBatches {
                batches: vec![BatchAcknowledgement {
                    batch_id: first.batch_id,
                    digest: first.digest
                },]
            })
        )
        .await?,
        AutomationAnswer::Acknowledged
    ));
    report(
        machine,
        &running,
        1,
        FiringOutcome::Uncertain {
            reason: "ongoing fixture work".to_owned().try_into()?,
        },
    )
    .await?;
    let second_due = GridTime::next_after(first_due)?.unix_millis();
    let mut next = definition;
    next.timing = at(second_due)?;
    create(machine, next).await?;
    advance(client, second_due).await?;
    let (_, unacked) = one_batch(machine).await?;
    Ok((
        running,
        unacked,
        client
            .automation_clock(AutomationClockControl::Observe)
            .await?
            .now,
    ))
}

async fn after_prune(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    foreign: &mut NativeBoardSession,
    service: &str,
    running: &FiringEntry,
    unacked: &FrozenAutomationBatch,
) -> ConformanceResult<()> {
    assert!(matches!(
        call(
            machine,
            AutomationCall::List(AutomationPageRequest {
                cursor: None,
                limit: 100.try_into()?,
            })
        )
        .await?,
        AutomationAnswer::List { .. }
    ));
    let raw = fixture::automation(client, &running.automation_id, None).await?;
    let firing = raw
        .get("firings")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first())
        .ok_or("retained Firing missing")?;
    assert!(
        firing
            .get("batchId")
            .is_some_and(serde_json::Value::is_null),
        "ACKed Batch was not pruned"
    );
    assert_eq!(
        firing
            .get("reportServiceId")
            .and_then(serde_json::Value::as_str),
        Some(service)
    );
    let batches = raw
        .get("batches")
        .and_then(serde_json::Value::as_array)
        .ok_or("raw custody missing")?;
    assert_eq!(
        batches.len(),
        1,
        "retention lost unACKed custody or retained ACKed custody"
    );
    assert_eq!(
        batches
            .first()
            .and_then(|batch| batch.get("batchId"))
            .and_then(serde_json::Value::as_str),
        Some(unacked.batch_id.as_str())
    );
    let custody = pull(machine).await?;
    assert_eq!(
        serde_json::to_value(&custody.batches)?,
        serde_json::to_value(vec![unacked])?,
        "unACKed custody expired or changed bytes"
    );
    report(
        machine,
        running,
        2,
        FiringOutcome::Uncertain {
            reason: "ongoing after retention".to_owned().try_into()?,
        },
    )
    .await?;
    let denied = call(
        foreign,
        AutomationCall::ReportWorkItems(ReportWorkItems::new(vec![WorkItemReport::Firing(
            FiringReport {
                firing_id: running.firing_id.clone(),
                seq: 3.try_into()?,
                outcome: FiringOutcome::Failed {
                    reason: "foreign report must not apply".to_owned().try_into()?,
                },
            },
        )])?),
    )
    .await?;
    assert!(
        matches!(
            denied,
            AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
        ),
        "foreign machine reported after Batch retention"
    );
    let shown = call(
        foreign,
        AutomationCall::FiringShow {
            firing_id: running.firing_id.clone(),
        },
    )
    .await?;
    let AutomationAnswer::Firing(snapshot) = shown else {
        return Err("any-machine Firing inspection refused".into());
    };
    assert!(snapshot.batch_id.is_none());
    assert_eq!(snapshot.report_seq.map(u64::from), Some(2));
    assert!(matches!(snapshot.outcome, FiringOutcome::Uncertain { .. }));
    Ok(())
}

fn at(milliseconds: i64) -> ConformanceResult<TimingRequest> {
    Ok(TimingRequest::At {
        at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(milliseconds)
            .ok_or("fixture due out of range")?
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            .try_into()?,
    })
}
async fn advance(client: &ConformanceClient, milliseconds: i64) -> ConformanceResult<()> {
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(milliseconds)
                    .ok_or("fixture advance out of range")?,
            ),
        })
        .await?;
    Ok(())
}
async fn create(
    machine: &mut NativeBoardSession,
    definition: ScheduleDefinition,
) -> ConformanceResult<()> {
    let answer = call(
        machine,
        AutomationCall::ScheduleCreate(ScheduleCreateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            definition,
        }),
    )
    .await?;
    assert!(
        matches!(answer, AutomationAnswer::Schedule(_)),
        "retention fixture refused: {answer:?}"
    );
    Ok(())
}
async fn one_batch(
    machine: &mut NativeBoardSession,
) -> ConformanceResult<(FiringEntry, FrozenAutomationBatch)> {
    let custody = pull(machine).await?;
    assert_eq!(custody.batches.len(), 1);
    let frozen = custody
        .batches
        .into_iter()
        .next()
        .ok_or("fixture Batch missing")?;
    let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
    let Some(WorkEntry::Firing(entry)) = batch.entries.entries().first() else {
        return Err("fixture Firing missing".into());
    };
    Ok((entry.clone(), frozen))
}
async fn report(
    machine: &mut NativeBoardSession,
    entry: &FiringEntry,
    sequence: u64,
    outcome: FiringOutcome,
) -> ConformanceResult<()> {
    let answer = call(
        machine,
        AutomationCall::ReportWorkItems(ReportWorkItems::new(vec![WorkItemReport::Firing(
            FiringReport {
                firing_id: entry.firing_id.clone(),
                seq: sequence.try_into()?,
                outcome,
            },
        )])?),
    )
    .await?;
    assert!(
        matches!(answer, AutomationAnswer::Reports(ref reports) if reports.accepted.len() == 1),
        "retention report refused: {answer:?}"
    );
    Ok(())
}
