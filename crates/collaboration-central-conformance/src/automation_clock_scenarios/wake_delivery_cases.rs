//! An undelivered Wake folds into its occupant; pause cannot chase its frozen Batch.
use super::{call, fixture, open_machine, pull};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationCall, BatchAcknowledgement,
    CatalogAutomation, CentralWakeSnapshot, FiringEntry, FiringMessageOrigin, FiringOutcome,
    FiringReport, FiringWork, GridTime, ReportWorkItems, WakeMutationRequest, WakeSendRequest,
    WakeShowRequest, WorkEntry, WorkItemReport,
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
    let now = client
        .automation_clock(AutomationClockControl::Observe)
        .await?
        .now;
    let due = GridTime::next_after(now.0.timestamp_millis())?.unix_millis();
    let request: WakeSendRequest = serde_json::from_value(serde_json::json!({
        "operationId":durable_steps::Uuid::now_v7(),"requestedBy":{"kind":"router"},"replace":false,
        "message":{"target":{"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"folding-wake-target"},
            "content":{"kind":"router","text":"frozen Wake body"},"delivery":"auto",
            "generationGuard":{"serviceEpoch":durable_steps::Uuid::now_v7(),"generation":9}},
        "timing":{"kind":"interval","seconds":600},"expiry":{"kind":"after","seconds":7200}
    }))?;
    let AutomationAnswer::Wake(wake) = call(machine, AutomationCall::WakeSend(request)).await?
    else {
        return Err("folding Wake refused".into());
    };
    advance(client, due).await?;
    let custody = pull(machine).await?;
    assert_eq!(custody.batches.len(), 1);
    let frozen = custody.batches.first().ok_or("Wake Batch missing")?;
    let first = entry(&frozen.frozen_bytes)?;
    let expected = FiringWork::Wake {
        created_by: wake.definition.created_by.clone(),
        timing: wake.definition.timing.clone(),
        anchor_at: wake.definition.anchor_at.clone(),
        created_at: wake.definition.created_at.clone(),
        target: wake.definition.message.target.clone(),
        origin: FiringMessageOrigin::Router,
        delivery: wake.definition.message.delivery,
        generation_guard: wake.definition.message.generation_guard.clone(),
        expires_at: wake
            .definition
            .expires_at
            .clone()
            .ok_or("Wake expiry missing")?,
    };
    assert_eq!(
        serde_json::to_value(&first.work)?,
        serde_json::to_value(expected)?
    );
    assert_eq!(first.revision, wake.definition.change_id);
    assert_eq!(first.due_at.unix_millis(), due);
    advance(client, due + 1_200_000).await?;
    let raw = fixture::automation(client, &first.automation_id, None).await?;
    assert!(
        raw.get("waitingFiringId")
            .is_some_and(serde_json::Value::is_null),
        "Wake invented a Schedule-style waiter"
    );
    let firings = raw
        .get("firings")
        .and_then(serde_json::Value::as_array)
        .ok_or("Wake rows missing")?;
    assert_eq!(
        firings.len(),
        1,
        "undelivered Wake admitted instead of folding"
    );
    assert_eq!(
        firings
            .first()
            .ok_or("Wake row missing after exact count")?
            .get("foldedThroughMs")
            .and_then(serde_json::Value::as_i64),
        Some(due + 1_200_000)
    );
    let resent = pull(machine).await?;
    assert_eq!(
        serde_json::to_value(&resent)?,
        serde_json::to_value(&custody)?,
        "Wake fold changed frozen executable bytes"
    );
    let paused = call(machine, AutomationCall::WakePause(mutation(&wake)?)).await?;
    let AutomationAnswer::Mutation {
        automation: CatalogAutomation::Wake(paused),
        already_sent,
    } = paused
    else {
        return Err("sent Wake pause refused".into());
    };
    assert_eq!(paused.state, collaboration_protocol::WakeState::Paused);
    assert!(paused.next_due_at.is_none());
    assert!(
        already_sent.contains(&first.firing_id),
        "pause hid its already-sent cutoff"
    );
    assert_eq!(
        serde_json::to_value(pull(machine).await?)?,
        serde_json::to_value(&custody)?
    );
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
                firing_id: first.firing_id.clone(),
                seq: 1.try_into()?,
                outcome: FiringOutcome::Delivered,
            },
        )])?),
    )
    .await?;
    assert!(matches!(reported,AutomationAnswer::Reports(ref reports) if reports.accepted.len()==1));
    let shown = call(
        machine,
        AutomationCall::WakeShow(WakeShowRequest {
            wakeup_id: wake.definition.wakeup_id.clone(),
        }),
    )
    .await?;
    let AutomationAnswer::Wake(shown) = shown else {
        return Err("delivered Wake could not be inspected".into());
    };
    assert!(
        matches!(shown.first_firing.as_ref(),Some(row) if row.firing_id==first.firing_id&&matches!(row.outcome,FiringOutcome::Delivered))
    );
    let resumed = call(machine, AutomationCall::WakeResume(mutation(&wake)?)).await?;
    let AutomationAnswer::Mutation {
        automation: CatalogAutomation::Wake(resumed),
        ..
    } = resumed
    else {
        return Err("Wake resume refused".into());
    };
    let next_due = due + 1_800_000;
    assert_eq!(
        resumed
            .next_due_at
            .as_ref()
            .map(|time| String::from(time.clone()).parse::<chrono::DateTime<chrono::Utc>>())
            .transpose()?
            .map(|time| time.timestamp_millis()),
        Some(next_due)
    );
    advance(client, next_due).await?;
    let next = pull(machine).await?;
    assert_eq!(next.batches.len(), 1);
    let next = next.batches.first().ok_or("resumed Wake Batch missing")?;
    let second = entry(&next.frozen_bytes)?;
    assert_ne!(second.firing_id, first.firing_id);
    assert_ne!(next.batch_id, frozen.batch_id);
    assert_eq!(second.automation_id, first.automation_id);
    assert_eq!(second.due_at.unix_millis(), next_due);
    assert_eq!(second.revision, resumed.definition.change_id);
    let raw = fixture::automation(client, &first.automation_id, None).await?;
    assert_eq!(
        raw.get("firings")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    println!(
        "AUTOMATION_WAKE_DELIVERY_OBSERVATION {}",
        serde_json::json!({
            "automationId":first.automation_id,"firstFiringId":first.firing_id,"secondFiringId":second.firing_id,
            "firstDueMs":due,"foldedThroughMs":due+1_200_000,"nextDueMs":next_due,
            "frozenFullHeader":true,"pauseAlreadySent":true,"undeliveredCount":1,
        })
    );
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "The exact one-entry assertion is the conformance oracle; malformed fixture bytes propagate as errors."
)]
fn entry(bytes: &[u8]) -> ConformanceResult<FiringEntry> {
    let batch: AutomationBatch = serde_json::from_slice(bytes)?;
    assert_eq!(batch.entries.entries().len(), 1);
    match batch.entries.entries().first() {
        Some(WorkEntry::Firing(entry)) => Ok(entry.clone()),
        _ => Err("Wake Firing missing".into()),
    }
}
fn mutation(wake: &CentralWakeSnapshot) -> ConformanceResult<WakeMutationRequest> {
    Ok(WakeMutationRequest {
        operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
        wakeup_id: wake.definition.wakeup_id.clone(),
    })
}
async fn advance(client: &ConformanceClient, milliseconds: i64) -> ConformanceResult<()> {
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(milliseconds)
                    .ok_or("Wake advance invalid")?,
            ),
        })
        .await?;
    Ok(())
}
