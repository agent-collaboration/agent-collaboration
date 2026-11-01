//! Faults and settlement exercise the retained waiting entry through normal machine calls.
use super::{call, fixture, pull};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationCall, AutomationPageRequest,
    BatchAcknowledgement, CatalogAutomation, FetchContents, FiringEntry, FiringOutcome,
    FiringReport, FiringSnapshot, FrozenAutomationBatch, ReportWorkItems, ScheduleDefinition,
    ScheduleEnableRequest, ScheduleSnapshot, ScheduleUpdateRequest, WorkEntry, WorkItemReport,
};

pub(super) async fn run(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    schedule: &ScheduleSnapshot,
    first: &FiringEntry,
    frozen: &FrozenAutomationBatch,
) -> ConformanceResult<()> {
    let fetched = call(
        machine,
        AutomationCall::FetchContents(FetchContents {
            contents: vec![first.content.clone()],
        }),
    )
    .await?;
    let AutomationAnswer::Contents(contents) = fetched else {
        return Err("content fetch refused".into());
    };
    assert_eq!(contents.len(), 1);
    assert_eq!(
        contents.first().ok_or("content missing")?.reference,
        first.content
    );
    assert_eq!(
        contents.first().ok_or("content missing")?.text.as_str(),
        "Frozen instruction for the first central deadline"
    );
    let ack = AckAutomationBatches {
        batches: vec![BatchAcknowledgement {
            batch_id: frozen.batch_id.clone(),
            digest: frozen.digest.clone(),
        }],
    };
    assert!(matches!(
        call(machine, AutomationCall::AckAutomationBatches(ack.clone())).await?,
        AutomationAnswer::Acknowledged
    ));
    assert!(matches!(
        call(machine, AutomationCall::AckAutomationBatches(ack)).await?,
        AutomationAnswer::Acknowledged
    ));
    assert!(
        pull(machine).await?.batches.is_empty(),
        "ACKed custody was returned again"
    );
    assert!(matches!(
        show(machine, first).await?.outcome,
        FiringOutcome::Received
    ));
    let run_id = durable_steps::Uuid::now_v7().to_string().try_into()?;
    reports(
        machine,
        vec![
            FiringReport {
                firing_id: first.firing_id.clone(),
                seq: 3.try_into()?,
                outcome: FiringOutcome::RunStarted { run_id },
            },
            FiringReport {
                firing_id: first.firing_id.clone(),
                seq: 2.try_into()?,
                outcome: FiringOutcome::Unknown,
            },
            FiringReport {
                firing_id: first.firing_id.clone(),
                seq: 3.try_into()?,
                outcome: FiringOutcome::Delivered,
            },
        ],
    )
    .await?;
    let observed = show(machine, first).await?;
    assert_eq!(observed.report_seq.map(u64::from), Some(3));
    assert!(
        matches!(observed.outcome, FiringOutcome::RunStarted { .. }),
        "older or duplicate report regressed outcome"
    );
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime("2026-10-31T10:25:00Z".parse()?),
        })
        .await?;
    assert!(
        pull(machine).await?.batches.is_empty(),
        "a waiting Schedule was sent while occupied"
    );
    let before_fault = fixture::automation(client, &first.automation_id, None).await?;
    let waiting_id = before_fault
        .get("waitingFiringId")
        .and_then(serde_json::Value::as_str)
        .ok_or("waiting Firing missing")?;
    let rows = before_fault
        .get("firings")
        .and_then(serde_json::Value::as_array)
        .ok_or("Firing rows missing")?;
    assert_eq!(
        rows.len(),
        2,
        "later due should fold into one waiting Firing"
    );
    let waiting = rows
        .iter()
        .find(|row| row.get("firingId").and_then(serde_json::Value::as_str) == Some(waiting_id))
        .ok_or("waiting row missing")?;
    assert_eq!(
        waiting
            .get("foldedThroughMs")
            .and_then(serde_json::Value::as_i64),
        Some(
            RegistryTime("2026-10-31T10:25:00Z".parse()?)
                .0
                .timestamp_millis()
        )
    );
    let retained: FiringEntry = serde_json::from_str(
        waiting
            .get("pendingEntryJson")
            .and_then(serde_json::Value::as_str)
            .ok_or("pending payload missing")?,
    )?;
    fixture::automation(
        client,
        &first.automation_id,
        Some("{deliberately unreadable".into()),
    )
    .await?;
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    let shown = call(
        machine,
        AutomationCall::ScheduleShow(collaboration_protocol::ScheduleShowRequest {
            schedule_id: schedule.schedule_id.clone(),
        }),
    )
    .await?;
    assert!(
        matches!(shown, AutomationAnswer::Faulted(ref fault) if fault.automation_id == first.automation_id),
        "faulted Show decoded or fabricated the corrupt body: {shown:?}"
    );
    let listed = call(
        machine,
        AutomationCall::List(AutomationPageRequest {
            cursor: None,
            limit: 100.try_into()?,
        }),
    )
    .await?;
    let AutomationAnswer::List { automations, .. } = listed else {
        return Err("faulted list refused".into());
    };
    assert!(automations.iter().any(|item| matches!(item, CatalogAutomation::Faulted(fault) if fault.automation_id == first.automation_id)));
    let faulted = fixture::automation(client, &first.automation_id, None).await?;
    assert!(
        faulted
            .get("nextDueMs")
            .is_some_and(serde_json::Value::is_null),
        "fault retained an eligible due deadline"
    );
    assert!(
        faulted
            .get("faultJson")
            .is_some_and(serde_json::Value::is_string)
    );
    assert_eq!(
        faulted.get("waitingFiringId"),
        before_fault.get("waitingFiringId")
    );
    reports(
        machine,
        vec![FiringReport {
            firing_id: first.firing_id.clone(),
            seq: 5.try_into()?,
            outcome: FiringOutcome::Failed {
                reason: "R0 settled".to_owned().try_into()?,
            },
        }],
    )
    .await?;
    let released = pull(machine).await?;
    assert_eq!(
        released.batches.len(),
        1,
        "fault stranded or doubled waiting work"
    );
    let batch: AutomationBatch = serde_json::from_slice(
        &released
            .batches
            .first()
            .ok_or("released Batch missing")?
            .frozen_bytes,
    )?;
    let Some(WorkEntry::Firing(entry)) = batch.entries.entries().first() else {
        return Err("released Firing missing".into());
    };
    assert_eq!(
        serde_json::to_value(entry)?,
        serde_json::to_value(&retained)?,
        "release fabricated work from the corrupt definition"
    );
    let settled = fixture::automation(client, &first.automation_id, None).await?;
    assert!(
        settled
            .get("waitingFiringId")
            .is_some_and(serde_json::Value::is_null)
    );
    assert!(
        settled
            .get("faultJson")
            .is_some_and(serde_json::Value::is_string)
    );
    let pending_after = settled
        .get("firings")
        .and_then(serde_json::Value::as_array)
        .ok_or("settled Firings missing")?
        .iter()
        .find(|row| {
            row.get("firingId").and_then(serde_json::Value::as_str)
                == Some(retained.firing_id.as_str())
        })
        .ok_or("released row missing")?;
    assert!(
        pending_after
            .get("pendingEntryJson")
            .is_some_and(serde_json::Value::is_null),
        "cutoff left a second executable truth"
    );
    reports(
        machine,
        vec![
            FiringReport {
                firing_id: first.firing_id.clone(),
                seq: 5.try_into()?,
                outcome: FiringOutcome::Unknown,
            },
            FiringReport {
                firing_id: first.firing_id.clone(),
                seq: 6.try_into()?,
                outcome: FiringOutcome::Unknown,
            },
        ],
    )
    .await?;
    assert_eq!(
        serde_json::to_value(pull(machine).await?)?,
        serde_json::to_value(&released)?,
        "duplicate settlement released twice"
    );
    let terminal = show(machine, first).await?;
    assert!(
        matches!(terminal.outcome, FiringOutcome::Failed { .. }),
        "terminal outcome was reopened"
    );
    assert_eq!(terminal.report_seq.map(u64::from), Some(6));
    repair_and_cancel(
        client,
        machine,
        schedule,
        &retained,
        &before_fault,
        &released,
    )
    .await
}

async fn repair_and_cancel(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    schedule: &ScheduleSnapshot,
    occupant: &FiringEntry,
    original: &serde_json::Value,
    custody: &collaboration_protocol::AutomationBatches,
) -> ConformanceResult<()> {
    let mut definition: ScheduleDefinition = schedule.definition.clone();
    definition.effort = Some("medium".into());
    let repair = call(
        machine,
        AutomationCall::ScheduleUpdate(ScheduleUpdateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            schedule_id: schedule.schedule_id.clone(),
            expected_change_id: schedule.change_id.clone(),
            definition,
        }),
    )
    .await?;
    assert!(
        matches!(
            repair,
            AutomationAnswer::Mutation {
                automation: CatalogAutomation::Schedule(_),
                ..
            }
        ),
        "valid guarded repair refused: {repair:?}"
    );
    let repaired = fixture::automation(client, &occupant.automation_id, None).await?;
    for field in [
        "anchorAtMs",
        "createdAtMs",
        "updatedAtMs",
        "executionMode",
        "importedContinuityJson",
    ] {
        assert_eq!(
            repaired.get(field),
            original.get(field),
            "repair changed header {field}"
        );
    }
    assert!(
        repaired
            .get("faultJson")
            .is_some_and(serde_json::Value::is_null)
    );
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime("2026-10-31T10:30:00Z".parse()?),
        })
        .await?;
    let pending = fixture::automation(client, &occupant.automation_id, None).await?;
    assert!(
        pending
            .get("waitingFiringId")
            .is_some_and(serde_json::Value::is_string)
    );
    let disabled = call(
        machine,
        AutomationCall::ScheduleDisable(ScheduleEnableRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            schedule_id: schedule.schedule_id.clone(),
        }),
    )
    .await?;
    let AutomationAnswer::Mutation { already_sent, .. } = disabled else {
        return Err("disable refused".into());
    };
    assert!(
        already_sent.contains(&occupant.firing_id),
        "post-cutoff work was hidden"
    );
    reports(
        machine,
        vec![FiringReport {
            firing_id: occupant.firing_id.clone(),
            seq: 1.try_into()?,
            outcome: FiringOutcome::Failed {
                reason: "R1 settled".to_owned().try_into()?,
            },
        }],
    )
    .await?;
    assert_eq!(
        serde_json::to_value(pull(machine).await?)?,
        serde_json::to_value(custody)?,
        "cancelled waiting Firing crossed the cutoff"
    );
    let ack = AckAutomationBatches {
        batches: custody
            .batches
            .iter()
            .map(|batch| BatchAcknowledgement {
                batch_id: batch.batch_id.clone(),
                digest: batch.digest.clone(),
            })
            .collect(),
    };
    assert!(matches!(
        call(machine, AutomationCall::AckAutomationBatches(ack)).await?,
        AutomationAnswer::Acknowledged
    ));
    assert!(pull(machine).await?.batches.is_empty());
    Ok(())
}

async fn show(
    machine: &mut NativeBoardSession,
    entry: &FiringEntry,
) -> ConformanceResult<FiringSnapshot> {
    match call(
        machine,
        AutomationCall::FiringShow {
            firing_id: entry.firing_id.clone(),
        },
    )
    .await?
    {
        AutomationAnswer::Firing(snapshot) => Ok(snapshot),
        other => Err(format!("FiringShow refused: {other:?}").into()),
    }
}
async fn reports(
    machine: &mut NativeBoardSession,
    reports: Vec<FiringReport>,
) -> ConformanceResult<()> {
    let count = reports.len();
    match call(
        machine,
        AutomationCall::ReportWorkItems(ReportWorkItems::new(
            reports.into_iter().map(WorkItemReport::Firing).collect(),
        )?),
    )
    .await?
    {
        AutomationAnswer::Reports(ack) => {
            assert_eq!(
                ack.accepted.len(),
                count,
                "duplicate/older report was not acknowledged"
            );
            Ok(())
        }
        other => Err(format!("reports refused: {other:?}").into()),
    }
}
