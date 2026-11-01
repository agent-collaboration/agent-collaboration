//! A large real catalog produces stable chunks and accepts bounded report envelopes.
use super::{call, fixture, pull};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationBatches, AutomationCall,
    BatchAcknowledgement, FiringEntry, FiringOutcome, FiringReport, PullAutomationBatches,
    ReportWorkItems, ScheduleCreateRequest, ScheduleDefinition, ScheduleShowRequest, TimingRequest,
    WorkEntry, WorkItemReport,
};
use std::collections::BTreeSet;

pub(super) async fn run(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    template: &ScheduleDefinition,
) -> ConformanceResult<()> {
    let mut expected = BTreeSet::new();
    let mut bad = None;
    for ordinal in 0..251 {
        let mut definition = template.clone();
        definition.timing = TimingRequest::At {
            at: "2026-10-31T10:35:00Z".to_owned().try_into()?,
        };
        let answer = call(
            machine,
            AutomationCall::ScheduleCreate(ScheduleCreateRequest {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                definition,
            }),
        )
        .await?;
        let AutomationAnswer::Schedule(snapshot) = answer else {
            return Err(format!("large fixture create {ordinal} refused: {answer:?}").into());
        };
        if ordinal == 250 {
            bad = Some(snapshot.schedule_id);
        } else {
            assert!(expected.insert(snapshot.schedule_id.as_str().to_owned()));
        }
    }
    let bad = bad.ok_or("bad fixture missing")?;
    let bad_automation = bad.as_str().to_owned().try_into()?;
    fixture::automation(
        client,
        &bad_automation,
        Some("{large fixture invalid definition".into()),
    )
    .await?;
    let before = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    let due = RegistryTime("2026-10-31T10:35:00Z".parse()?);
    let fired = client
        .automation_clock(AutomationClockControl::Advance { now: due })
        .await?;
    assert!(
        fired.callbacks.len() > before.callbacks.len(),
        "large fixture had no actual callback"
    );
    let custody = pull(machine).await?;
    assert_eq!(
        custody.batches.len(),
        3,
        "250 due entries lost or duplicated a chunk"
    );
    let mut batch_ids = BTreeSet::new();
    let mut firing_ids = BTreeSet::new();
    let mut observed = BTreeSet::new();
    let mut counts = Vec::new();
    let mut entries = Vec::new();
    for frozen in &custody.batches {
        assert!(batch_ids.insert(frozen.batch_id.as_str().to_owned()));
        let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
        assert_eq!(batch.batch_id, frozen.batch_id);
        counts.push(batch.entries.entries().len());
        assert!(batch.entries.entries().len() <= 100);
        for item in batch.entries.entries() {
            let WorkEntry::Firing(entry) = item else {
                return Err("large clock fixture changed work kind".into());
            };
            assert!(firing_ids.insert(entry.firing_id.as_str().to_owned()));
            assert!(observed.insert(entry.automation_id.as_str().to_owned()));
            assert_eq!(entry.due_at.unix_millis(), due.0.timestamp_millis());
            entries.push(entry.clone());
        }
    }
    counts.sort_unstable();
    assert_eq!(counts, vec![50, 100, 100]);
    assert_eq!(
        observed, expected,
        "large clock fixture dropped or manufactured an Automation"
    );
    assert_eq!(firing_ids.len(), 250);
    let fault = call(
        machine,
        AutomationCall::ScheduleShow(ScheduleShowRequest { schedule_id: bad }),
    )
    .await?;
    assert!(
        matches!(fault, AutomationAnswer::Faulted(ref fault) if fault.automation_id == bad_automation),
        "invalid Automation stopped healthy ones or was not reported: {fault:?}"
    );
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    assert_eq!(
        serde_json::to_value(pull(machine).await?)?,
        serde_json::to_value(&custody)?,
        "duplicate callback changed or released unACKed chunks"
    );
    pagination(machine, &custody).await?;
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
    assert!(
        pull(machine).await?.batches.is_empty(),
        "ACKed chunks stayed pending"
    );
    reports(machine, &entries).await?;
    println!(
        "AUTOMATION_LARGE_BATCH_OBSERVATION 250 distinct Firings; 3 stable chunks 100/100/50; invalid sibling isolated; 250 reports in 3 calls"
    );
    Ok(())
}

async fn pagination(
    machine: &mut NativeBoardSession,
    custody: &AutomationBatches,
) -> ConformanceResult<()> {
    let first = custody.batches.first().ok_or("first chunk missing")?;
    let budget = serde_json::to_vec(&AutomationAnswer::Batches(AutomationBatches {
        batches: vec![first.clone()],
        next: Some(first.batch_id.clone()),
    }))?
    .len()
    .try_into()?;
    let AutomationAnswer::Batches(page) = call(
        machine,
        AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None,
            max_bytes: budget,
            work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
        }),
    )
    .await?
    else {
        return Err("bounded custody page refused".into());
    };
    assert_eq!(page.batches.len(), 1);
    assert_eq!(page.next.as_ref(), Some(&first.batch_id));
    assert!(serde_json::to_vec(&AutomationAnswer::Batches(page.clone()))?.len() <= budget as usize);
    let AutomationAnswer::Batches(tail) = call(
        machine,
        AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: page.next,
            max_bytes: 1_048_576,
            work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
        }),
    )
    .await?
    else {
        return Err("custody continuation refused".into());
    };
    assert_eq!(tail.batches.len(), 2);
    assert!(tail.next.is_none());
    assert_eq!(
        serde_json::to_value(&tail.batches)?,
        serde_json::to_value(custody.batches.iter().skip(1).collect::<Vec<_>>())?
    );
    Ok(())
}

async fn reports(
    machine: &mut NativeBoardSession,
    entries: &[FiringEntry],
) -> ConformanceResult<()> {
    let mut reports = Vec::new();
    for entry in entries.iter().take(50) {
        let run: collaboration_protocol::RunId =
            durable_steps::Uuid::now_v7().to_string().try_into()?;
        for (seq, outcome) in [
            (1, FiringOutcome::Received),
            (
                2,
                FiringOutcome::RunStarted {
                    run_id: run.clone(),
                },
            ),
            (
                3,
                FiringOutcome::Uncertain {
                    reason: "acceptance answer missing".to_owned().try_into()?,
                },
            ),
            (4, FiringOutcome::RunStarted { run_id: run }),
            (
                5,
                FiringOutcome::Failed {
                    reason: "fixture settled".to_owned().try_into()?,
                },
            ),
        ] {
            reports.push(WorkItemReport::Firing(FiringReport {
                firing_id: entry.firing_id.clone(),
                seq: seq.try_into()?,
                outcome,
            }));
        }
    }
    assert_eq!(reports.len(), 250);
    let mut calls = 0;
    for chunk in reports.chunks(100) {
        let AutomationAnswer::Reports(ack) = call(
            machine,
            AutomationCall::ReportWorkItems(ReportWorkItems::new(chunk.to_vec())?),
        )
        .await?
        else {
            return Err("bounded report envelope refused".into());
        };
        assert_eq!(ack.accepted.len(), chunk.len());
        calls += 1;
    }
    assert_eq!(calls, 3);
    for entry in entries.iter().take(50) {
        let AutomationAnswer::Firing(snapshot) = call(
            machine,
            AutomationCall::FiringShow {
                firing_id: entry.firing_id.clone(),
            },
        )
        .await?
        else {
            return Err("reported Firing not readable".into());
        };
        assert_eq!(snapshot.report_seq.map(u64::from), Some(5));
        assert!(matches!(snapshot.outcome, FiringOutcome::Failed { .. }));
    }
    Ok(())
}
