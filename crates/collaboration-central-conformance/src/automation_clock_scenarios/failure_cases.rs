//! Actual late SQL failures retain owed work; recovery uses the ordinary catalog path.
use super::{AutomationClockPlatform, call, fixture, pull};
use crate::{
    AutomationAlarmResult, AutomationClockControl, AutomationClockObservation, ConformanceClient,
    ConformanceResult, RuntimeControl, native_board_session::NativeBoardSession,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationCall, AutomationId,
    AutomationPageRequest, BatchAcknowledgement, GridTime, InstructionCreateParams,
    ScheduleCreateRequest, ScheduleDefinition, TimingRequest, WorkEntry,
};

pub(super) async fn run(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    template: &ScheduleDefinition,
    platform: AutomationClockPlatform,
) -> ConformanceResult<()> {
    let (id, due) = create(client, machine, template).await?;
    let baseline = capture_baseline(client, &id, due).await?;
    arm(client, 1).await?;
    let failed = advance(client, due).await?;
    assert_failure(&failed)?;
    assert_owed(client, &id, &baseline).await?;
    let retry = failed
        .next_alarm_ms
        .ok_or("failed callback omitted its platform retry")?;
    let completed = advance(client, retry).await?;
    assert!(matches!(
        completed.callbacks.last().map(|receipt| &receipt.result),
        Some(AutomationAlarmResult::Completed)
    ));
    check_and_ack(machine, &id, due).await?;

    for trigger in ["create", "list", "pull"] {
        let (id, due) = create(client, machine, template).await?;
        let baseline = capture_baseline(client, &id, due).await?;
        arm(client, 6).await?;
        let mut failed = advance(client, due).await?;
        for ordinal in 0..6 {
            assert_failure(&failed)?;
            assert_owed(client, &id, &baseline).await?;
            if ordinal < 5 {
                let retry = failed
                    .next_alarm_ms
                    .ok_or("retry exhausted before six actual failures")?;
                failed = advance(client, retry).await?;
            }
        }
        match platform {
            AutomationClockPlatform::Workerd => {
                assert!(
                    failed.next_alarm_ms.is_none(),
                    "simulated Cloudflare alarm did not exhaust after six failures"
                );
            }
            AutomationClockPlatform::Tokio => {
                // Tokio's real retry owner is unbounded. Inject platform loss
                // after observing six genuine failures; do not label it native
                // Cloudflare exhaustion or change the production owner policy.
                client
                    .automation_clock(AutomationClockControl::RemoveAlarm)
                    .await?;
            }
        }
        let lost = client
            .automation_clock(AutomationClockControl::Observe)
            .await?;
        assert!(
            lost.platform_alarm_before_control_ms.is_none(),
            "Observe repaired the lost alarm"
        );
        assert!(lost.next_alarm_ms.is_none());
        match trigger {
            "create" => {
                assert!(matches!(
                    call(
                        machine,
                        AutomationCall::InstructionCreate(InstructionCreateParams {
                            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                            text: "recovery create".to_owned().try_into()?,
                        })
                    )
                    .await?,
                    AutomationAnswer::Instruction(_)
                ));
            }
            "list" => {
                assert!(matches!(
                    call(
                        machine,
                        AutomationCall::List(AutomationPageRequest {
                            cursor: None,
                            limit: 100.try_into()?
                        })
                    )
                    .await?,
                    AutomationAnswer::List { .. }
                ));
            }
            "pull" => {
                pull(machine).await?;
            }
            _ => return Err("unknown recovery trigger".into()),
        }
        client
            .automation_clock(AutomationClockControl::Advance { now: lost.now })
            .await?;
        check_and_ack(machine, &id, due).await?;
        println!(
            "AUTOMATION_FAILURE_RECOVERY {trigger}: six actual rollbacks; passive lost-alarm observation; exactly one recovered Firing"
        );
    }
    Ok(())
}

async fn arm(client: &ConformanceClient, count: u8) -> ConformanceResult<()> {
    crate::composed_client::control(
        client,
        ObjectAddress::Fleet,
        RuntimeControl::AutomationStoreFailures {
            after_writes: 5.try_into()?,
            failures: count.try_into()?,
        },
    )
    .await?;
    Ok(())
}
async fn advance(
    client: &ConformanceClient,
    milliseconds: i64,
) -> ConformanceResult<AutomationClockObservation> {
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(milliseconds)
                    .ok_or("retry time is invalid")?,
            ),
        })
        .await
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions are the conformance oracle; missing fixture observations propagate as errors."
)]
fn assert_failure(observation: &AutomationClockObservation) -> ConformanceResult<()> {
    let receipt = observation
        .callbacks
        .last()
        .ok_or("failed callback not recorded")?;
    println!(
        "AUTOMATION_CALLBACK_RECEIPT {}",
        serde_json::to_string(receipt)?
    );
    match &receipt.result {
        AutomationAlarmResult::Failed { message } => {
            assert!(!message.is_empty(), "failure receipt lacks actual error")
        }
        other @ AutomationAlarmResult::Completed => {
            return Err(
                format!("injected SQL failure did not fail the real handler: {other:?}").into(),
            );
        }
    }
    assert!(
        receipt.after_handler_alarm_ms.is_none(),
        "failed transaction retained a handler alarm before platform retry"
    );
    Ok(())
}
const ROLLBACK_FIELDS: &[&str] = &[
    "nextDueMs",
    "expiresAtMs",
    "admittedThroughMs",
    "occupyingFiringId",
    "waitingFiringId",
    "externalRunId",
    "firings",
    "batches",
    "revision",
    "definitionJson",
];
async fn capture_baseline(
    client: &ConformanceClient,
    id: &AutomationId,
    due: i64,
) -> ConformanceResult<serde_json::Value> {
    let snapshot = fixture::automation(client, id, None).await?;
    for field in ROLLBACK_FIELDS {
        if snapshot.get(*field).is_none() {
            return Err(format!("rollback baseline omitted {field}").into());
        }
    }
    assert!(
        snapshot
            .get("batches")
            .is_some_and(serde_json::Value::is_array)
    );
    assert_eq!(
        snapshot
            .get("nextDueMs")
            .and_then(serde_json::Value::as_i64),
        Some(due)
    );
    println!(
        "AUTOMATION_ROLLBACK_BASELINE {}",
        rollback_record(id, &snapshot)?
    );
    Ok(snapshot)
}
async fn assert_owed(
    client: &ConformanceClient,
    id: &AutomationId,
    baseline: &serde_json::Value,
) -> ConformanceResult<()> {
    let row = fixture::automation(client, id, None).await?;
    for field in ROLLBACK_FIELDS {
        assert_eq!(
            row.get(*field),
            baseline.get(*field),
            "transient failure changed {field}"
        );
    }
    assert_eq!(
        row.get("firings")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0),
        "transient failure committed a Firing"
    );
    assert!(
        row.get("admittedThroughMs")
            .is_some_and(serde_json::Value::is_null)
    );
    assert!(
        row.get("faultJson").is_some_and(serde_json::Value::is_null),
        "transient failure was recorded as permanent"
    );
    assert!(row.get("nextDueMs").is_some_and(serde_json::Value::is_i64));
    println!(
        "AUTOMATION_ROLLBACK_OBSERVATION {}",
        rollback_record(id, &row)?
    );
    Ok(())
}

fn rollback_record(
    id: &AutomationId,
    row: &serde_json::Value,
) -> ConformanceResult<serde_json::Value> {
    use sha2::{Digest, Sha256};
    let mut relevant = std::collections::BTreeMap::new();
    for field in ROLLBACK_FIELDS {
        relevant.insert(
            *field,
            row.get(*field)
                .ok_or("rollback record omitted a required field")?,
        );
    }
    Ok(serde_json::json!({
        "automationId": id, "nextDueMs": row.get("nextDueMs"),
        "admittedThroughMs": row.get("admittedThroughMs"),
        "occupyingFiringId": row.get("occupyingFiringId"),
        "waitingFiringId": row.get("waitingFiringId"),
        "firingCount": row.get("firings").and_then(serde_json::Value::as_array).map(Vec::len),
        "batchCount": row.get("batches").and_then(serde_json::Value::as_array).map(Vec::len),
        "snapshotDigest": format!("{:x}", Sha256::digest(serde_json::to_vec(&relevant)?)),
    }))
}
async fn create(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    template: &ScheduleDefinition,
) -> ConformanceResult<(AutomationId, i64)> {
    let now = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    let due = GridTime::next_after(now.now.0.timestamp_millis())?.unix_millis();
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(due)
        .ok_or("due time is invalid")?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut definition = template.clone();
    definition.timing = TimingRequest::At {
        at: timestamp.try_into()?,
    };
    let answer = call(
        machine,
        AutomationCall::ScheduleCreate(ScheduleCreateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            definition,
        }),
    )
    .await?;
    match answer {
        AutomationAnswer::Schedule(snapshot) => {
            Ok((snapshot.schedule_id.as_str().to_owned().try_into()?, due))
        }
        other => Err(format!("failure fixture create refused: {other:?}").into()),
    }
}
async fn check_and_ack(
    machine: &mut NativeBoardSession,
    id: &AutomationId,
    original_due: i64,
) -> ConformanceResult<()> {
    let custody = pull(machine).await?;
    assert_eq!(
        custody.batches.len(),
        1,
        "recovery lost or duplicated a Batch"
    );
    let frozen = custody.batches.first().ok_or("recovery Batch missing")?;
    let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
    assert_eq!(batch.entries.entries().len(), 1);
    let Some(WorkEntry::Firing(entry)) = batch.entries.entries().first() else {
        return Err("recovered Firing missing".into());
    };
    assert_eq!(entry.automation_id, *id);
    assert_eq!(
        entry.due_at.unix_millis(),
        original_due,
        "recovery shifted the owed due slot"
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
                },]
            })
        )
        .await?,
        AutomationAnswer::Acknowledged
    ));
    Ok(())
}
