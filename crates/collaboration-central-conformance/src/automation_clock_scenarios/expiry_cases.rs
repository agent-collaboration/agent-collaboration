//! The shared min-only timer preserves expiry when admission is paused or faulted.
use super::{call, fixture, open_machine, pull};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationId, AutomationRefusal, CatalogAutomation,
    CentralWakeSnapshot, ExpiryRequest, GridTime, MessageContent, MessageDelivery, SavedMessage,
    TimingRequest, WakeCreator, WakeMutationRequest, WakeSendRequest, WakeShowRequest, WakeState,
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
    pull(machine).await?; // Advertise the actual target's machine-work capability.
    for armed in [true, false] {
        let (request, due, expiry) = make_request(client, service).await?;
        let original = wake(call(machine, AutomationCall::WakeSend(request)).await?)?;
        if !armed {
            client
                .automation_clock(AutomationClockControl::RemoveAlarm)
                .await?;
        }
        let paused = call(
            machine,
            AutomationCall::WakePause(WakeMutationRequest {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                wakeup_id: original.definition.wakeup_id.clone(),
            }),
        )
        .await?;
        let AutomationAnswer::Mutation {
            automation: CatalogAutomation::Wake(paused),
            already_sent,
        } = paused
        else {
            return Err("Wake pause did not return its central state".into());
        };
        assert!(already_sent.is_empty());
        assert_eq!(paused.state, WakeState::Paused);
        assert!(paused.next_due_at.is_none());
        let observed = client
            .automation_clock(AutomationClockControl::Observe)
            .await?;
        assert_eq!(
            observed.next_alarm_ms,
            Some(if armed { due } else { expiry })
        );
        let at_due = advance(client, due).await?;
        assert_eq!(
            at_due.callbacks.len(),
            observed.callbacks.len() + usize::from(armed),
            "paused Wake repeated a stale alarm or invented an unarmed callback"
        );
        assert_eq!(at_due.next_alarm_ms, Some(expiry));
        assert!(pull(machine).await?.batches.is_empty());
        let quiet = advance(client, due + 300_000).await?;
        assert_eq!(
            quiet.callbacks.len(),
            at_due.callbacks.len(),
            "paused Wake polled before expiry"
        );
        let ended = advance(client, expiry).await?;
        assert_eq!(ended.callbacks.len(), quiet.callbacks.len() + 1);
        assert!(ended.next_alarm_ms.is_none());
        let shown = wake(
            call(
                machine,
                AutomationCall::WakeShow(WakeShowRequest {
                    wakeup_id: original.definition.wakeup_id.clone(),
                }),
            )
            .await?,
        )?;
        assert_eq!(shown.state, WakeState::Expired);
        assert!(shown.first_firing.is_none());
        let raw = fixture::automation(client, &automation_id(&original)?, None).await?;
        assert!(raw.get("nextDueMs").is_some_and(serde_json::Value::is_null));
        assert_eq!(
            raw.get("expiresAtMs").and_then(serde_json::Value::as_i64),
            Some(expiry)
        );
        assert_eq!(
            raw.get("firings")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(0)
        );
        println!(
            "AUTOMATION_WAKE_EXPIRY_OBSERVATION {}",
            serde_json::json!({
                "case": if armed { "one-stale-callback" } else { "unarmed-expiry-only" },
                "wakeId": original.definition.wakeup_id, "expiryMs": expiry,
                "callbacksAtDue": at_due.callbacks.len() - observed.callbacks.len(),
                "callbacksAtExpiry": ended.callbacks.len() - quiet.callbacks.len(),
                "state": shown.state,
            })
        );
        // The next loop's replace=false creation proves the old slot is free.
    }
    let (request, due, expiry) = make_request(client, service).await?;
    let faulted = wake(call(machine, AutomationCall::WakeSend(request)).await?)?;
    corrupt(client, &faulted).await?;
    let fault = call(
        machine,
        AutomationCall::WakeShow(WakeShowRequest {
            wakeup_id: faulted.definition.wakeup_id.clone(),
        }),
    )
    .await?;
    assert!(
        matches!(fault, AutomationAnswer::Faulted(ref row) if row.automation_id == automation_id(&faulted)?)
    );
    let observed = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    assert_eq!(observed.next_alarm_ms, Some(expiry));
    let quiet = advance(client, due + 300_000).await?;
    assert_eq!(
        quiet.callbacks.len(),
        observed.callbacks.len(),
        "faulted Wake kept its stale due"
    );
    let ended = advance(client, expiry).await?;
    assert_eq!(ended.callbacks.len(), quiet.callbacks.len() + 1);
    let raw = fixture::automation(client, &automation_id(&faulted)?, None).await?;
    assert_eq!(
        raw.get("state").and_then(serde_json::Value::as_str),
        Some("expired")
    );
    assert!(
        raw.get("faultJson")
            .is_some_and(serde_json::Value::is_string)
    );
    assert!(pull(machine).await?.batches.is_empty());
    println!(
        "AUTOMATION_WAKE_EXPIRY_OBSERVATION {}",
        serde_json::json!({
            "case":"faulted-body-independent-expiry","wakeId":faulted.definition.wakeup_id,
            "expiryMs":expiry,"callbacksAtExpiry":ended.callbacks.len()-quiet.callbacks.len(),
            "state":raw.get("state"),
        })
    );
    replacement(client, machine, service).await?;
    invalid_expiry_keeps_sibling_admission(client, machine, service).await
}

async fn replacement(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    service: &str,
) -> ConformanceResult<()> {
    let (request, _, _) = make_request(client, service).await?;
    let original = wake(call(machine, AutomationCall::WakeSend(request)).await?)?;
    corrupt(client, &original).await?;
    let (denied, _, _) = make_request(client, service).await?;
    assert!(
        matches!(
            call(machine, AutomationCall::WakeSend(denied)).await?,
            AutomationAnswer::Refused(AutomationRefusal::WakeExists { .. })
        ),
        "faulted Wake lost its slot"
    );
    let foreign_service = durable_steps::Uuid::now_v7().to_string();
    let mut foreign = open_machine(client, &foreign_service).await?;
    let (mut foreign_request, _, _) = make_request(client, service).await?;
    foreign_request.replace = true;
    let denied = call(&mut foreign, AutomationCall::WakeSend(foreign_request)).await;
    let closed = foreign.close().await;
    assert!(
        matches!(
            denied?,
            AutomationAnswer::Refused(AutomationRefusal::WakeExists { .. })
        ),
        "foreign Router impersonated faulted Wake creator"
    );
    closed?;
    let (mut replacement, _, _) = make_request(client, service).await?;
    replacement.replace = true;
    let repaired = wake(call(machine, AutomationCall::WakeSend(replacement)).await?)?;
    assert_ne!(repaired.definition.wakeup_id, original.definition.wakeup_id);
    let old = fixture::automation(client, &automation_id(&original)?, None).await?;
    assert_eq!(
        old.get("state").and_then(serde_json::Value::as_str),
        Some("replaced")
    );
    assert!(
        old.get("faultJson")
            .is_some_and(serde_json::Value::is_string),
        "replacement erased fault history"
    );
    assert_eq!(repaired.state, WakeState::Active);
    println!(
        "AUTOMATION_WAKE_REPLACEMENT_OBSERVATION {}",
        serde_json::json!({
            "oldWakeId": original.definition.wakeup_id, "newWakeId": repaired.definition.wakeup_id,
            "oldState":old.get("state"), "foreignCreatorRefused":true,
        })
    );
    call(
        machine,
        AutomationCall::WakeCancel(WakeMutationRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            wakeup_id: repaired.definition.wakeup_id,
        }),
    )
    .await?;
    Ok(())
}

async fn invalid_expiry_keeps_sibling_admission(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    service: &str,
) -> ConformanceResult<()> {
    use collaboration_central_core::AutomationFixtureMutation;
    use collaboration_protocol::{
        AutomationBatch, ExecutionDestination, InstructionCreateParams, ScheduleCreateRequest,
        ScheduleDefinition, WorkEntry,
    };
    let (request, due, _) = make_request(client, service).await?;
    let bad = wake(call(machine, AutomationCall::WakeSend(request)).await?)?;
    let instruction = super::require_instruction(
        call(
            machine,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                text: "valid sibling beside corrupt expiry"
                    .to_owned()
                    .try_into()?,
            }),
        )
        .await?,
    )?;
    let answer = call(
        machine,
        AutomationCall::ScheduleCreate(ScheduleCreateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            definition: ScheduleDefinition {
                instruction_id: instruction.instruction_id,
                timing: TimingRequest::At {
                    at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(due)
                        .ok_or("sibling due invalid")?
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                        .try_into()?,
                },
                enabled: true,
                destination: ExecutionDestination::OwnedThread {
                    target: bad.definition.message.target.clone(),
                    cwd: "/workspace".into(),
                },
                execution_timeout_seconds: None,
                model: None,
                effort: Some("high".into()),
            },
        }),
    )
    .await?;
    let AutomationAnswer::Schedule(sibling) = answer else {
        return Err("valid expiry sibling refused".into());
    };
    let bad_id = automation_id(&bad)?;
    fixture::mutate(
        client,
        &bad_id,
        AutomationFixtureMutation::WakeExpiry {
            expires_at_ms: i64::MAX,
        },
    )
    .await?;
    let fired = advance(client, due).await?;
    println!(
        "AUTOMATION_INVALID_EXPIRY_CALLBACK {}",
        serde_json::to_string(&fired)?
    );
    assert!(
        fired.callbacks.last().is_some_and(|receipt| matches!(
            receipt.result,
            crate::AutomationAlarmResult::Completed
        )),
        "one malformed stored expiry rolled back the whole Fleet evaluation"
    );
    let raw = fixture::automation(client, &bad_id, None).await?;
    let error: AutomationRefusal = serde_json::from_str(
        raw.get("faultJson")
            .and_then(serde_json::Value::as_str)
            .ok_or("bad expiry was not recorded as a row fault")?,
    )?;
    assert!(
        matches!(error, AutomationRefusal::InvalidRecord {ref field} if field=="expires_at_ms")
    );
    assert_eq!(
        raw.get("expiresAtMs").and_then(serde_json::Value::as_i64),
        Some(i64::MAX),
        "invalid expiry was silently coerced"
    );
    assert!(raw.get("nextDueMs").is_some_and(serde_json::Value::is_null));
    assert_eq!(
        raw.get("firings")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    let custody = pull(machine).await?;
    assert_eq!(
        custody.batches.len(),
        1,
        "valid sibling did not retain its owed Batch"
    );
    let frozen = custody
        .batches
        .first()
        .ok_or("valid sibling Batch missing")?;
    let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
    assert_eq!(batch.entries.entries().len(), 1);
    let Some(WorkEntry::Firing(entry)) = batch.entries.entries().first() else {
        return Err("valid sibling Firing missing".into());
    };
    assert_eq!(entry.automation_id.as_str(), sibling.schedule_id.as_str());
    assert_eq!(entry.due_at.unix_millis(), due);
    let quiet = super::restart_cases::drain_notice_turn(client, &sibling.schedule_id, None).await?;
    assert!(
        quiet.next_alarm_ms.is_none(),
        "unrepresentable expiry became a timer"
    );
    println!(
        "AUTOMATION_INVALID_EXPIRY_OBSERVATION {}",
        serde_json::json!({
            "faultedWakeId":bad_id,"validSiblingId":sibling.schedule_id,"firingId":entry.firing_id,
            "dueMs":due,"error":error,"invalidExpiryPreserved":true,"validSiblingAdmissions":1,
        })
    );
    Ok(())
}
async fn make_request(
    client: &ConformanceClient,
    service: &str,
) -> ConformanceResult<(WakeSendRequest, i64, i64)> {
    let now = client
        .automation_clock(AutomationClockControl::Observe)
        .await?
        .now;
    let due = GridTime::next_after(now.0.timestamp_millis())?.unix_millis();
    let expiry = due.checked_add(420_000).ok_or("Wake expiry overflow")?;
    let request = WakeSendRequest {
        operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
        requested_by: WakeCreator::Router {},
        replace: false,
        message: SavedMessage {
            target: serde_json::from_value(serde_json::json!({
                "endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"expiry-target"
            }))?,
            content: MessageContent::Router {
                text: "independent expiry".to_owned().try_into()?,
            },
            delivery: MessageDelivery::Auto,
            generation_guard: None,
        },
        timing: TimingRequest::Interval {
            seconds: 600.try_into()?,
            first_due: None,
        },
        expiry: ExpiryRequest::At {
            at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(expiry)
                .ok_or("invalid expiry")?
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                .try_into()?,
        },
    };
    Ok((request, due, expiry))
}
async fn corrupt(client: &ConformanceClient, wake: &CentralWakeSnapshot) -> ConformanceResult<()> {
    fixture::automation(
        client,
        &automation_id(wake)?,
        Some("{unreadable Wake body".into()),
    )
    .await?;
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    Ok(())
}
fn automation_id(wake: &CentralWakeSnapshot) -> ConformanceResult<AutomationId> {
    Ok(wake.definition.wakeup_id.as_str().to_owned().try_into()?)
}
fn wake(answer: AutomationAnswer) -> ConformanceResult<CentralWakeSnapshot> {
    match answer {
        AutomationAnswer::Wake(wake) => Ok(wake),
        other => Err(format!("expected central Wake: {other:?}").into()),
    }
}
async fn advance(
    client: &ConformanceClient,
    milliseconds: i64,
) -> ConformanceResult<crate::AutomationClockObservation> {
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(
                chrono::DateTime::from_timestamp_millis(milliseconds)
                    .ok_or("invalid Wake control time")?,
            ),
        })
        .await
}
