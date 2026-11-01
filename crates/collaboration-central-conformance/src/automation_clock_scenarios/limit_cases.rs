//! Exact byte/counter bounds traverse real central entry and stored-row owners.
use super::{call, fixture, open_machine, pull, require_instruction, same_answer};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationCall, AutomationId, AutomationRefusal,
    AutomationValidationError, ExecutionDestination, FiringEntry, FiringOutcome, FiringReport,
    GridTime, InstructionCreateParams, InstructionShowParams, ReportWorkItems,
    ScheduleCreateRequest, ScheduleDefinition, TimingRequest, WakeSendRequest, WorkEntry,
    WorkItemReport,
    limits::{self, LimitName},
};
use serde_json::{Value, json};
mod stored;

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
    let text = "🦀".repeat(limits::INSTRUCTION_MAX_BYTES / 4);
    let instruction = require_instruction(
        call(
            machine,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: operation()?,
                text: text.clone().try_into()?,
            }),
        )
        .await?,
    )?;
    assert_eq!(
        instruction.text.as_str().len(),
        limits::INSTRUCTION_MAX_BYTES
    );
    assert!(
        matches!(call(machine,AutomationCall::InstructionShow(InstructionShowParams {
        instruction_id:instruction.instruction_id.clone(),
    })).await?,AutomationAnswer::Instruction(ref row) if row.text.as_str()==text)
    );
    let now = client
        .automation_clock(AutomationClockControl::Observe)
        .await?
        .now;
    let due = GridTime::next_after(now.0.timestamp_millis())?.unix_millis();
    let target = json!({"endpoint":{"serviceId":service,"endpointId":"a".repeat(limits::ENDPOINT_ID_MAX_BYTES)},
        "sessionId":"🦀".repeat(limits::SESSION_ID_MAX_BYTES/4)});
    let definition = ScheduleDefinition {
        instruction_id: instruction.instruction_id.clone(),
        timing: TimingRequest::Interval {
            seconds: 300.try_into()?,
            first_due: None,
        },
        enabled: true,
        destination: ExecutionDestination::OwnedThread {
            target: serde_json::from_value(target.clone())?,
            cwd: "/workspace".into(),
        },
        execution_timeout_seconds: None,
        model: None,
        effort: Some("high".into()),
    };
    let create = ScheduleCreateRequest {
        operation_id: operation()?,
        definition,
    };
    let original = call(machine, AutomationCall::ScheduleCreate(create.clone())).await?;
    let AutomationAnswer::Schedule(schedule) = &original else {
        return Err("exact central identity bounds refused".into());
    };
    let id: AutomationId = schedule.schedule_id.as_str().to_owned().try_into()?;
    let saved = fixture::automation(client, &id, None).await?;
    assert_eq!(saved.get("nextDueMs").and_then(Value::as_i64), Some(due));
    let mut largest = create.clone();
    largest.operation_id = operation()?;
    largest.definition.enabled = false;
    largest.definition.timing = TimingRequest::Interval {
        seconds: limits::DURATION_MAX_SECONDS.try_into()?,
        first_due: None,
    };
    assert!(matches!(
        call(machine, AutomationCall::ScheduleCreate(largest)).await?,
        AutomationAnswer::Schedule(_)
    ));
    let expiry_ms = now
        .0
        .timestamp_millis()
        .checked_add(i64::from(limits::WAKE_REPEAT_MAX_LIFETIME) * 1000)
        .ok_or("limit expiry overflow")?;
    let wake: WakeSendRequest = serde_json::from_value(json!({
        "operationId":operation()?,"requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":target,"content":{"kind":"humanUser","text":"🦀".repeat(limits::WAKE_MESSAGE_MAX_BYTES/4)},
            "delivery":"auto","generationGuard":{"serviceEpoch":durable_steps::Uuid::now_v7(),"generation":limits::JSON_SAFE_INTEGER_MAX}},
        "timing":{"kind":"interval","seconds":600},"expiry":{"kind":"at","at":timestamp(expiry_ms)?}
    }))?;
    let AutomationAnswer::Wake(wake) = call(machine, AutomationCall::WakeSend(wake)).await? else {
        return Err("exact Wake byte/lifetime/counter bounds refused".into());
    };
    assert_eq!(
        serde_json::to_value(&wake)?
            .pointer("/definition/message/content/text")
            .and_then(Value::as_str)
            .map(str::len),
        Some(limits::WAKE_MESSAGE_MAX_BYTES)
    );
    let furthest: WakeSendRequest = serde_json::from_value(json!({
        "operationId":operation()?,"requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":{"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"furthest-wake"},
            "content":{"kind":"humanUser","text":"furthest one-shot"},"delivery":"auto"},
        "timing":{"kind":"at","at":timestamp(now.0.timestamp_millis()+i64::from(limits::WAKE_ONE_SHOT_MAX_AHEAD)*1000)?},
        "expiry":{"kind":"none"}
    }))?;
    assert!(matches!(
        call(machine, AutomationCall::WakeSend(furthest)).await?,
        AutomationAnswer::Wake(_)
    ));
    // Both first-due choices are stored and no row is admitted before that boundary.
    assert!(pull(machine).await?.batches.is_empty());
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(due).ok_or("limit due invalid")?,
            ),
        })
        .await?;
    same_answer(
        call(machine, AutomationCall::ScheduleCreate(create)).await?,
        original.clone(),
    )?;
    let custody = pull(machine).await?;
    assert_eq!(custody.batches.len(), 1);
    let frozen = custody.batches.first().ok_or("limit Batch missing")?;
    let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
    assert_eq!(batch.entries.entries().len(), 2);
    let entry = batch
        .entries
        .entries()
        .iter()
        .find_map(|entry| match entry {
            WorkEntry::Firing(entry) if entry.automation_id == id => Some(entry),
            _ => None,
        })
        .ok_or("limit Schedule Firing missing")?;
    let admitted = fixture::automation(client, &id, None).await?;
    let firing = admitted
        .get("firings")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("firingId") == Some(&json!(entry.firing_id)))
        })
        .ok_or("admitted Firing row missing")?;
    assert_eq!(
        firing.get("reportSeq"),
        Some(&Value::Null),
        "unreported Firing must persist NULL"
    );
    assert!(
        matches!(call(machine, AutomationCall::FiringShow { firing_id: entry.firing_id.clone() }).await?,
        AutomationAnswer::Firing(snapshot) if snapshot.report_seq.is_none())
    );
    reports(machine, entry).await?;
    stored::run(
        client,
        machine,
        stored::Fixture {
            schedule,
            wake: &wake,
            instruction: &instruction,
            saved: &saved,
            entry,
            frozen,
            batch: &batch,
            expiry_ms,
        },
    )
    .await?;
    inventory();
    println!(
        "AUTOMATION_CENTRAL_LIMIT_RESULT exact multibyte text/identity bounds, max report sequence and envelope, stored corruption refusal passed"
    );
    Ok(())
}
async fn reports(machine: &mut NativeBoardSession, entry: &FiringEntry) -> ConformanceResult<()> {
    let report = WorkItemReport::Firing(FiringReport {
        firing_id: entry.firing_id.clone(),
        seq: limits::JSON_SAFE_INTEGER_MAX.try_into()?,
        outcome: FiringOutcome::Uncertain {
            reason: "🦀".repeat(limits::TEXT_MAX_BYTES / 4).try_into()?,
        },
    });
    let accepted = call(
        machine,
        AutomationCall::ReportWorkItems(ReportWorkItems::new(vec![
            report.clone();
            limits::REPORT_FLUSH_MAX
        ])?),
    )
    .await?;
    assert!(
        matches!(accepted,AutomationAnswer::Reports(ref rows) if rows.accepted.len()==limits::REPORT_FLUSH_MAX)
    );
    let raw = serde_json::to_value(report)?;
    for count in [0, limits::REPORT_FLUSH_MAX + 1] {
        limited(
            crate::automation_catalog_scenarios::raw_call(
                machine,
                json!({"kind":"reportWorkItems","value":{"reports":vec![raw.clone();count]}}),
            )
            .await?,
            LimitName::ReportFlushMax,
            count as u64,
        )?;
    }
    Ok(())
}
fn limited(answer: AutomationAnswer, limit: LimitName, value: u64) -> ConformanceResult<()> {
    match answer {
        AutomationAnswer::Refused(AutomationRefusal::Validation {
            error: AutomationValidationError::LimitExceeded(error),
        }) if error.limit == limit
            && error.value == value
            && error.minimum == limit.definition().minimum
            && error.maximum == limit.definition().maximum =>
        {
            Ok(())
        }
        other => Err(format!("central {limit} bound did not retain refusal: {other:?}").into()),
    }
}
fn invalid(answer: AutomationAnswer) -> ConformanceResult<()> {
    if matches!(
        answer,
        AutomationAnswer::Refused(AutomationRefusal::InvalidRecord { .. })
    ) {
        Ok(())
    } else {
        Err("corrupt stored bound was accepted or coerced".into())
    }
}
fn operation() -> ConformanceResult<collaboration_protocol::OperationId> {
    Ok(durable_steps::Uuid::now_v7().to_string().try_into()?)
}
fn timestamp(milliseconds: i64) -> ConformanceResult<String> {
    Ok(
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(milliseconds)
            .ok_or("limit timestamp invalid")?
            .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
    )
}
fn inventory() {
    for limit in limits::INVENTORY {
        let owner = match limit.name {
            LimitName::GridStep
            | LimitName::WakeMinPeriod
            | LimitName::WakeOneShotMaxAhead
            | LimitName::WakeRepeatMaxLifetime
            | LimitName::ScheduleMinInterval => {
                "M2 timing entry and Fleet definition decode; shared timing and stored-bound scenarios"
            }
            LimitName::BatchMaxEntries => "M2 250-entry real split and Fleet corrupt Batch decode",
            LimitName::ReportFlushMax => {
                "M2 real 100-report envelope and 0/101 refusal; no persisted report envelope"
            }
            LimitName::InstructionMaxBytes
            | LimitName::WakeMessageMaxBytes
            | LimitName::EndpointIdMaxBytes
            | LimitName::SessionIdMaxBytes
            | LimitName::TextMaxBytes
            | LimitName::JsonSafeInteger => {
                "M2 real exact boundary, oversized entry and corrupt Fleet row decode"
            }
            LimitName::WakeSlot => {
                "M2 real paused/faulted slot, authorized replacement and foreign refusal; unique slot relation"
            }
            LimitName::NotificationInterval | LimitName::NotificationListingMaxEntries => {
                "S10 notification owner; no M2 central value"
            }
            LimitName::ReportFlushWindow => "S5 receiver flusher; no M2 central timer/value",
        };
        println!(
            "AUTOMATION_LIMIT_OWNER {}",
            json!({"limit":limit.name,"owner":owner})
        );
    }
}
