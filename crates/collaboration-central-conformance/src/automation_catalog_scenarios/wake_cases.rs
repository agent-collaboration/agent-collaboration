//! The central owner enforces one active-or-paused wake and atomic replacement.
use super::{call, same_answer};
use crate::{ConformanceResult, native_board_session::NativeBoardSession};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, CentralWakeSnapshot, ExpiryRequest,
    MessageContent, MessageDelivery, SavedMessage, SessionRef, TimingRequest, WakeCreator,
    WakeMutationRequest, WakeSendRequest, WakeShowRequest, WakeState,
};

pub(super) async fn run(
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    target_service: &str,
) -> ConformanceResult<()> {
    let target: SessionRef = serde_json::from_value(
        serde_json::json!({"endpoint":{"serviceId":target_service,"endpointId":"codex-local"},"sessionId":"wake-target"}),
    )?;
    let request = WakeSendRequest {
        operation_id: operation()?,
        requested_by: WakeCreator::Router {},
        replace: false,
        message: SavedMessage {
            target: target.clone(),
            content: MessageContent::Router {
                text: "First central wake".to_owned().try_into()?,
            },
            delivery: MessageDelivery::Auto,
            generation_guard: None,
        },
        timing: TimingRequest::Interval {
            seconds: 600.try_into()?,
            first_due: None,
        },
        expiry: ExpiryRequest::After {
            seconds: 3601.try_into()?,
        },
    };
    let original = required(call(first, AutomationCall::WakeSend(request.clone())).await?)?;
    if original.state != WakeState::Active
        || original.first_firing.is_some()
        || original
            .next_due_at
            .as_ref()
            .map(|value| String::from(value.clone()))
            .as_deref()
            != Some("2026-10-31T10:05:00Z")
        || original
            .definition
            .expires_at
            .as_ref()
            .map(|value| String::from(value.clone()))
            .as_deref()
            != Some("2026-10-31T11:00:01Z")
    {
        return Err(
            "central wake lost its grid phase, off-grid expiry or central-only first Firing".into(),
        );
    }
    same_answer(
        call(
            second,
            AutomationCall::WakeShow(WakeShowRequest {
                wakeup_id: original.definition.wakeup_id.clone(),
            }),
        )
        .await?,
        AutomationAnswer::Wake(original.clone()),
    )?;
    let paused = mutation(
        call(
            second,
            AutomationCall::WakePause(WakeMutationRequest {
                operation_id: operation()?,
                wakeup_id: original.definition.wakeup_id.clone(),
            }),
        )
        .await?,
    )?;
    if paused.state != WakeState::Paused || paused.next_due_at.is_some() {
        return Err("paused Wake retained a deadline".into());
    }
    let mut replacement = request.clone();
    replacement.operation_id = operation()?;
    if !matches!(
        call(first, AutomationCall::WakeSend(replacement.clone())).await?,
        AutomationAnswer::Refused(AutomationRefusal::WakeExists {
            creator: WakeCreator::Router {}
        })
    ) {
        return Err("a paused Wake did not retain the target's slot".into());
    }
    replacement.operation_id = operation()?;
    replacement.replace = true;
    if !matches!(
        call(second, AutomationCall::WakeSend(replacement.clone())).await?,
        AutomationAnswer::Refused(AutomationRefusal::WakeExists {
            creator: WakeCreator::Router {}
        })
    ) {
        return Err("another installation impersonated the Router creator".into());
    }
    replacement.operation_id = operation()?;
    let replaced = required(call(first, AutomationCall::WakeSend(replacement.clone())).await?)?;
    if replaced.definition.wakeup_id == original.definition.wakeup_id
        || replaced.state != WakeState::Active
    {
        return Err("replacement did not create one new active Wake".into());
    }
    let ended = required(
        call(
            second,
            AutomationCall::WakeShow(WakeShowRequest {
                wakeup_id: original.definition.wakeup_id.clone(),
            }),
        )
        .await?,
    )?;
    if ended.state != WakeState::Cancelled || ended.next_due_at.is_some() {
        return Err("replacement did not atomically cancel the old Wake".into());
    }
    same_answer(
        call(first, AutomationCall::WakeSend(replacement.clone())).await?,
        AutomationAnswer::Wake(replaced.clone()),
    )?;
    same_answer(
        call(second, AutomationCall::WakeSend(request)).await?,
        AutomationAnswer::Wake(original),
    )?;
    let mut spoofed = replacement.clone();
    spoofed.operation_id = operation()?;
    spoofed.requested_by = WakeCreator::Session(SessionRef {
        endpoint: target.endpoint.clone(),
        session_id: "foreign-creator".to_owned().try_into()?,
    });
    if !matches!(
        call(first, AutomationCall::WakeSend(spoofed)).await?,
        AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
    ) {
        return Err("foreign declared creator passed the bound installation check".into());
    }
    let mut target_replacement = replacement;
    target_replacement.operation_id = operation()?;
    target_replacement.requested_by = WakeCreator::Session(target);
    target_replacement.message.content = MessageContent::HumanUser {
        text: "Target authorizes its own replacement"
            .to_owned()
            .try_into()?,
    };
    let owned =
        required(call(second, AutomationCall::WakeSend(target_replacement.clone())).await?)?;
    let resumed = mutation(
        call(
            first,
            AutomationCall::WakeResume(WakeMutationRequest {
                operation_id: operation()?,
                wakeup_id: owned.definition.wakeup_id.clone(),
            }),
        )
        .await?,
    )?;
    if resumed.state != WakeState::Active {
        return Err("active Wake resume changed its lifecycle".into());
    }
    let owner = replace_as_owner(first, second, target_replacement, &owned).await?;
    let cancelled = mutation(
        call(
            first,
            AutomationCall::WakeCancel(WakeMutationRequest {
                operation_id: operation()?,
                wakeup_id: owner.definition.wakeup_id,
            }),
        )
        .await?,
    )?;
    if cancelled.state != WakeState::Cancelled || cancelled.next_due_at.is_some() {
        return Err("cancel did not release the central Wake".into());
    }
    Ok(())
}

async fn replace_as_owner(
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    mut request: WakeSendRequest,
    previous: &CentralWakeSnapshot,
) -> ConformanceResult<CentralWakeSnapshot> {
    // The previous creator is the target Session on the other installation.
    // This request therefore needs the independent Owner authorization path.
    request.operation_id = operation()?;
    request.requested_by = WakeCreator::Owner {};
    request.replace = true;
    request.message.content = MessageContent::HumanUser {
        text: "Owner authorizes replacement of a session-created Wake"
            .to_owned()
            .try_into()?,
    };
    let answer = call(first, AutomationCall::WakeSend(request.clone())).await?;
    let AutomationAnswer::Wake(owner) = answer else {
        return Err(format!("owner replacement was denied: {answer:?}").into());
    };
    if owner.definition.wakeup_id == previous.definition.wakeup_id
        || owner.state != WakeState::Active
        || !matches!(owner.definition.created_by, WakeCreator::Owner {})
        || owner.definition.message.target != request.message.target
    {
        return Err("owner replacement did not create the distinct active Owner Wake".into());
    }
    let ended = required(
        call(
            second,
            AutomationCall::WakeShow(WakeShowRequest {
                wakeup_id: previous.definition.wakeup_id.clone(),
            }),
        )
        .await?,
    )?;
    if ended.state != WakeState::Cancelled || ended.next_due_at.is_some() {
        return Err("owner replacement left the previous Wake live".into());
    }
    same_answer(
        call(first, AutomationCall::WakeSend(request.clone())).await?,
        AutomationAnswer::Wake(owner.clone()),
    )?;
    same_answer(
        call(
            second,
            AutomationCall::WakeShow(WakeShowRequest {
                wakeup_id: owner.definition.wakeup_id.clone(),
            }),
        )
        .await?,
        AutomationAnswer::Wake(owner.clone()),
    )?;
    request.operation_id = operation()?;
    request.replace = false;
    if !matches!(
        call(second, AutomationCall::WakeSend(request)).await?,
        AutomationAnswer::Refused(AutomationRefusal::WakeExists {
            creator: WakeCreator::Owner {}
        })
    ) {
        return Err("the replacement Owner Wake did not own the target slot".into());
    }
    println!(
        "AUTOMATION_OWNER_REPLACEMENT {}",
        serde_json::json!({"oldWakeId":previous.definition.wakeup_id,"oldState":"cancelled",
            "newWakeId":owner.definition.wakeup_id,"newState":"active","creator":"owner",
            "sameTarget":true,"replayStable":true,"slotOwned":true})
    );
    Ok(owner)
}

fn required(answer: AutomationAnswer) -> ConformanceResult<CentralWakeSnapshot> {
    match answer {
        AutomationAnswer::Wake(snapshot) => Ok(snapshot),
        other => Err(format!("expected central Wake: {other:?}").into()),
    }
}
fn mutation(answer: AutomationAnswer) -> ConformanceResult<CentralWakeSnapshot> {
    match answer {
        AutomationAnswer::Mutation {
            automation: collaboration_protocol::CatalogAutomation::Wake(snapshot),
            already_sent,
        } if already_sent.is_empty() => Ok(snapshot),
        other => Err(format!("expected Wake mutation before cutoff: {other:?}").into()),
    }
}
fn operation() -> ConformanceResult<collaboration_protocol::OperationId> {
    Ok(durable_steps::Uuid::now_v7().to_string().try_into()?)
}
