//! Corrupt only one named real row, observe its production decoder, then restore.
use super::{invalid, limited, timestamp};
use crate::automation_clock_scenarios::{call, fixture, pull};
use crate::{ConformanceClient, ConformanceResult, native_board_session::NativeBoardSession};
use collaboration_central_core::AutomationFixtureMutation;
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationCall, AutomationId, AutomationKind,
    AutomationRefusal, AutomationTiming, AutomationValidationError, CentralWakeSnapshot,
    FiringEntry, FrozenAutomationBatch, InstructionShowParams, InstructionSnapshot,
    ScheduleShowRequest, ScheduleSnapshot, TimingRequest, WakeShowRequest, WorkEntry,
    limits::{self, LimitName},
};
use serde_json::{Value, json};

pub(super) struct Fixture<'a> {
    pub schedule: &'a ScheduleSnapshot,
    pub wake: &'a CentralWakeSnapshot,
    pub instruction: &'a InstructionSnapshot,
    pub saved: &'a Value,
    pub entry: &'a FiringEntry,
    pub frozen: &'a FrozenAutomationBatch,
    pub batch: &'a AutomationBatch,
    pub expiry_ms: i64,
}
pub(super) async fn run(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    fixture: Fixture<'_>,
) -> ConformanceResult<()> {
    definition_bounds(client, machine, &fixture).await?;
    content_bounds(client, machine, &fixture).await?;
    custody_bounds(client, machine, &fixture).await
}
async fn definition_bounds(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    case: &Fixture<'_>,
) -> ConformanceResult<()> {
    let id = &case.entry.automation_id;
    let original = case
        .saved
        .get("definitionJson")
        .and_then(Value::as_str)
        .ok_or("stored Schedule body missing")?;
    for (pointer, text, limit) in [
        (
            "/value/definition/destination/target/endpoint/endpointId",
            "a".repeat(limits::ENDPOINT_ID_MAX_BYTES + 1),
            LimitName::EndpointIdMaxBytes,
        ),
        (
            "/value/definition/destination/target/sessionId",
            "🦀".repeat(limits::SESSION_ID_MAX_BYTES / 4 + 1),
            LimitName::SessionIdMaxBytes,
        ),
    ] {
        let mut body: Value = serde_json::from_str(original)?;
        *body
            .pointer_mut(pointer)
            .ok_or("stored identity pointer missing")? = json!(text);
        fixture::automation(client, id, Some(body.to_string())).await?;
        limited(
            call(
                machine,
                AutomationCall::ScheduleShow(ScheduleShowRequest {
                    schedule_id: case.schedule.schedule_id.clone(),
                }),
            )
            .await?,
            limit,
            text.len() as u64,
        )?;
        fixture::automation(client, id, Some(original.into())).await?;
    }
    let mut body: Value = serde_json::from_str(original)?;
    *body
        .pointer_mut("/value/definition/timing/seconds")
        .ok_or("stored interval missing")? = json!(450);
    fixture::automation(client, id, Some(body.to_string())).await?;
    assert!(matches!(
        call(
            machine,
            AutomationCall::ScheduleShow(ScheduleShowRequest {
                schedule_id: case.schedule.schedule_id.clone()
            })
        )
        .await?,
        AutomationAnswer::Refused(AutomationRefusal::Validation {
            error: AutomationValidationError::OffGrid { .. }
        })
    ));
    fixture::automation(client, id, Some(original.into())).await?;
    let wake_id: AutomationId = case
        .wake
        .definition
        .wakeup_id
        .as_str()
        .to_owned()
        .try_into()?;
    let raw = fixture::automation(client, &wake_id, None).await?;
    let original = raw
        .get("definitionJson")
        .and_then(Value::as_str)
        .ok_or("stored Wake body missing")?;
    for (pointer, value, limit, expected) in [
        (
            "/value/timingRequest/seconds",
            json!(300),
            LimitName::WakeMinPeriod,
            300,
        ),
        (
            "/value/generationGuard/generation",
            json!(limits::JSON_SAFE_INTEGER_MAX + 1),
            LimitName::JsonSafeInteger,
            limits::JSON_SAFE_INTEGER_MAX + 1,
        ),
    ] {
        let mut body: Value = serde_json::from_str(original)?;
        *body
            .pointer_mut(pointer)
            .ok_or("stored Wake bound pointer missing")? = value;
        fixture::automation(client, &wake_id, Some(body.to_string())).await?;
        limited(show_wake(machine, case.wake).await?, limit, expected)?;
        fixture::automation(client, &wake_id, Some(original.into())).await?;
    }
    let created = raw
        .get("createdAtMs")
        .and_then(Value::as_i64)
        .ok_or("Wake creation header missing")?;
    let mut body: Value = serde_json::from_str(original)?;
    let timing = TimingRequest::At {
        at: timestamp(created + 86_700_000)?.try_into()?,
    };
    *body
        .pointer_mut("/value/timingRequest")
        .ok_or("stored timing request missing")? = serde_json::to_value(&timing)?;
    *body
        .pointer_mut("/value/timing")
        .ok_or("stored timing missing")? = serde_json::to_value(AutomationTiming::resolve(
        &timing,
        AutomationKind::Wake,
        created,
    )?)?;
    fixture::automation(client, &wake_id, Some(body.to_string())).await?;
    limited(
        show_wake(machine, case.wake).await?,
        LimitName::WakeOneShotMaxAhead,
        86_700,
    )?;
    fixture::automation(client, &wake_id, Some(original.into())).await?;
    fixture::mutate(
        client,
        &wake_id,
        AutomationFixtureMutation::WakeExpiry {
            expires_at_ms: created + 604_801_000,
        },
    )
    .await?;
    limited(
        show_wake(machine, case.wake).await?,
        LimitName::WakeRepeatMaxLifetime,
        604_801,
    )?;
    fixture::mutate(
        client,
        &wake_id,
        AutomationFixtureMutation::WakeExpiry {
            expires_at_ms: case.expiry_ms,
        },
    )
    .await?;
    Ok(())
}
async fn content_bounds(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    case: &Fixture<'_>,
) -> ConformanceResult<()> {
    let text = "🦀".repeat(limits::INSTRUCTION_MAX_BYTES / 4 + 1);
    fixture::mutate(
        client,
        &case.entry.automation_id,
        AutomationFixtureMutation::ContentText {
            content_id: case.entry.content.content_id.clone(),
            text: text.clone(),
        },
    )
    .await?;
    limited(
        call(
            machine,
            AutomationCall::InstructionShow(InstructionShowParams {
                instruction_id: case.instruction.instruction_id.clone(),
            }),
        )
        .await?,
        LimitName::InstructionMaxBytes,
        text.len() as u64,
    )?;
    fixture::mutate(
        client,
        &case.entry.automation_id,
        AutomationFixtureMutation::ContentText {
            content_id: case.entry.content.content_id.clone(),
            text: case.instruction.text.as_str().into(),
        },
    )
    .await?;
    let wake_id: AutomationId = case
        .wake
        .definition
        .wakeup_id
        .as_str()
        .to_owned()
        .try_into()?;
    let entry = case
        .batch
        .entries
        .entries()
        .iter()
        .find_map(|entry| match entry {
            WorkEntry::Firing(entry) if entry.automation_id == wake_id => Some(entry),
            _ => None,
        })
        .ok_or("Wake content reference missing")?;
    let text = "🦀".repeat(limits::WAKE_MESSAGE_MAX_BYTES / 4 + 1);
    fixture::mutate(
        client,
        &wake_id,
        AutomationFixtureMutation::ContentText {
            content_id: entry.content.content_id.clone(),
            text: text.clone(),
        },
    )
    .await?;
    limited(
        show_wake(machine, case.wake).await?,
        LimitName::WakeMessageMaxBytes,
        text.len() as u64,
    )?;
    let original = serde_json::to_value(case.wake)?;
    let text = original
        .pointer("/definition/message/content/text")
        .and_then(Value::as_str)
        .ok_or("Wake original content missing")?
        .into();
    fixture::mutate(
        client,
        &wake_id,
        AutomationFixtureMutation::ContentText {
            content_id: entry.content.content_id.clone(),
            text,
        },
    )
    .await?;
    Ok(())
}
async fn custody_bounds(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    case: &Fixture<'_>,
) -> ConformanceResult<()> {
    let reason = "🦀".repeat(limits::TEXT_MAX_BYTES / 4 + 1);
    fixture::mutate(
        client,
        &case.entry.automation_id,
        AutomationFixtureMutation::FiringOutcome {
            firing_id: case.entry.firing_id.clone(),
            report_seq: limits::JSON_SAFE_INTEGER_MAX,
            outcome_json: json!({"state":"uncertain","reason":reason}).to_string(),
        },
    )
    .await?;
    limited(
        show_firing(machine, case.entry).await?,
        LimitName::TextMaxBytes,
        reason.len() as u64,
    )?;
    let outcome_json =
        json!({"state":"uncertain","reason":"🦀".repeat(limits::TEXT_MAX_BYTES/4)}).to_string();
    fixture::mutate(
        client,
        &case.entry.automation_id,
        AutomationFixtureMutation::FiringOutcome {
            firing_id: case.entry.firing_id.clone(),
            report_seq: limits::JSON_SAFE_INTEGER_MAX + 1,
            outcome_json: outcome_json.clone(),
        },
    )
    .await?;
    limited(
        show_firing(machine, case.entry).await?,
        LimitName::JsonSafeInteger,
        limits::JSON_SAFE_INTEGER_MAX + 1,
    )?;
    fixture::mutate(
        client,
        &case.entry.automation_id,
        AutomationFixtureMutation::FiringOutcome {
            firing_id: case.entry.firing_id.clone(),
            report_seq: limits::JSON_SAFE_INTEGER_MAX,
            outcome_json,
        },
    )
    .await?;
    let mut body = serde_json::to_value(case.batch)?;
    let entry = body
        .get("entries")
        .and_then(Value::as_array)
        .and_then(|entries| entries.first())
        .ok_or("Batch entries missing")?
        .clone();
    *body.get_mut("entries").ok_or("Batch entries missing")? =
        json!(vec![entry; limits::BATCH_MAX_ENTRIES + 1]);
    fixture::mutate(
        client,
        &case.entry.automation_id,
        AutomationFixtureMutation::BatchBytes {
            batch_id: case.frozen.batch_id.clone(),
            frozen_bytes: body.to_string(),
        },
    )
    .await?;
    invalid(
        call(
            machine,
            AutomationCall::PullAutomationBatches(collaboration_protocol::PullAutomationBatches {
                after: None,
                max_bytes: 1_048_576,
                work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
            }),
        )
        .await?,
    )?;
    fixture::mutate(
        client,
        &case.entry.automation_id,
        AutomationFixtureMutation::BatchBytes {
            batch_id: case.frozen.batch_id.clone(),
            frozen_bytes: String::from_utf8(case.frozen.frozen_bytes.clone())?,
        },
    )
    .await?;
    let restored = pull(machine).await?;
    assert_eq!(restored.batches.len(), 1);
    assert_eq!(
        restored
            .batches
            .first()
            .ok_or("restored Batch missing")?
            .frozen_bytes,
        case.frozen.frozen_bytes
    );
    Ok(())
}
async fn show_wake(
    machine: &mut NativeBoardSession,
    wake: &CentralWakeSnapshot,
) -> ConformanceResult<AutomationAnswer> {
    call(
        machine,
        AutomationCall::WakeShow(WakeShowRequest {
            wakeup_id: wake.definition.wakeup_id.clone(),
        }),
    )
    .await
}
async fn show_firing(
    machine: &mut NativeBoardSession,
    entry: &FiringEntry,
) -> ConformanceResult<AutomationAnswer> {
    call(
        machine,
        AutomationCall::FiringShow {
            firing_id: entry.firing_id.clone(),
        },
    )
    .await
}
