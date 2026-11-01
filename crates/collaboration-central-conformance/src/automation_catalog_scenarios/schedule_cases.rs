//! Cross-machine schedule mutations retain their operation and revision guards.
use super::{call, require_instruction, same_answer};
use crate::{ConformanceResult, native_board_session::NativeBoardSession};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, ExecutionDestination,
    InstructionCreateParams, MachineWorkProtocol, PullAutomationBatches, ScheduleCreateRequest,
    ScheduleDefinition, ScheduleEnableRequest, ScheduleShowRequest, ScheduleSnapshot,
    ScheduleUpdateRequest, TimingRequest,
};

pub(super) async fn run(
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    target_service: &str,
) -> ConformanceResult<ScheduleSnapshot> {
    let instruction = require_instruction(
        call(
            first,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: operation()?,
                text: "Schedule's current instruction".to_owned().try_into()?,
            }),
        )
        .await?,
    )?;
    let definition = ScheduleDefinition {
        instruction_id: instruction.instruction_id,
        timing: TimingRequest::Interval {
            seconds: 600.try_into()?,
            first_due: None,
        },
        enabled: true,
        destination: ExecutionDestination::OwnedThread {
            target: serde_json::from_value(serde_json::json!({
                "endpoint":{"serviceId":target_service,"endpointId":"codex-local"},
                "sessionId":"schedule-target"
            }))?,
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
    let capability_refusal = call(first, AutomationCall::ScheduleCreate(create.clone())).await?;
    if !matches!(&capability_refusal,
        AutomationAnswer::Refused(AutomationRefusal::TargetNeedsUpgrade {service_id})
        if String::from(service_id.clone()) == target_service)
    {
        return Err(format!(
            "missing capability did not return targetNeedsUpgrade: {capability_refusal:?}"
        )
        .into());
    }
    for relative_path in ["workspace", "./workspace", "../workspace", ""] {
        let mut relative = create.clone();
        relative.operation_id = operation()?;
        let ExecutionDestination::OwnedThread { cwd, .. } = &mut relative.definition.destination
        else {
            return Err("expected owned-thread fixture".into());
        };
        *cwd = relative_path.into();
        if !matches!(call(first,AutomationCall::ScheduleCreate(relative)).await?,AutomationAnswer::Refused(AutomationRefusal::InvalidRequest {field,reason:collaboration_protocol::AutomationRequestConstraint::AbsolutePathRequired}) if field=="destination.cwd")
        {
            return Err("central accepted a relative destination path".into());
        }
    }
    let AutomationAnswer::Batches(empty) = call(
        second,
        AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None,
            max_bytes: 1_048_576,
            work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        }),
    )
    .await?
    else {
        return Err("machine capability pull failed".into());
    };
    if !empty.batches.is_empty() || empty.next.is_some() {
        return Err("fresh machine had work".into());
    }
    // A refused operation remains refused after the target changes capability.
    if !matches!(
        call(first, AutomationCall::ScheduleCreate(create.clone())).await?,
        AutomationAnswer::Refused(AutomationRefusal::TargetNeedsUpgrade { .. })
    ) {
        return Err("a replayed refusal was re-evaluated".into());
    }
    let mut eligible = create;
    eligible.operation_id = operation()?;
    let original =
        require_schedule(call(first, AutomationCall::ScheduleCreate(eligible.clone())).await?)?;
    if original
        .next_due_at
        .as_ref()
        .map(|value| String::from(value.clone()))
        .as_deref()
        != Some("2026-10-31T10:05:00Z")
    {
        return Err("default interval did not choose the strict next grid boundary".into());
    }
    same_answer(
        call(
            second,
            AutomationCall::ScheduleShow(ScheduleShowRequest {
                schedule_id: original.schedule_id.clone(),
            }),
        )
        .await?,
        AutomationAnswer::Schedule(original.clone()),
    )?;
    let mut update = ScheduleUpdateRequest {
        operation_id: operation()?,
        schedule_id: original.schedule_id.clone(),
        expected_change_id: original.change_id.clone(),
        definition: original.definition.clone(),
    };
    update.definition.timing = TimingRequest::At {
        at: "2026-10-31T10:15:00Z".to_owned().try_into()?,
    };
    let changed =
        require_mutation(call(second, AutomationCall::ScheduleUpdate(update.clone())).await?)?;
    if changed.change_id == original.change_id
        || changed.created_at != original.created_at
        || changed.anchor_at != original.anchor_at
        || changed
            .next_due_at
            .as_ref()
            .map(|value| String::from(value.clone()))
            .as_deref()
            != Some("2026-10-31T10:15:00Z")
    {
        return Err("schedule update lost its identity, anchor, or replacement timing".into());
    }
    same_answer(
        call(first, AutomationCall::ScheduleCreate(eligible)).await?,
        AutomationAnswer::Schedule(original.clone()),
    )?;
    let mut stale = update.clone();
    stale.operation_id = operation()?;
    if !matches!(call(first, AutomationCall::ScheduleUpdate(stale)).await?,
        AutomationAnswer::Refused(AutomationRefusal::ScheduleChangeConflict {current_change_id}) if current_change_id == changed.change_id)
    {
        return Err("schedule stale guard was not enforced".into());
    }
    let disabled = require_mutation(
        call(
            first,
            AutomationCall::ScheduleDisable(ScheduleEnableRequest {
                operation_id: operation()?,
                schedule_id: changed.schedule_id.clone(),
            }),
        )
        .await?,
    )?;
    if disabled.definition.enabled || disabled.next_due_at.is_some() {
        return Err("disabled schedule retained a deadline".into());
    }
    let enabled = require_mutation(
        call(
            second,
            AutomationCall::ScheduleEnable(ScheduleEnableRequest {
                operation_id: operation()?,
                schedule_id: changed.schedule_id.clone(),
            }),
        )
        .await?,
    )?;
    if !enabled.definition.enabled || enabled.next_due_at.is_none() {
        return Err("enabled schedule lost its deadline".into());
    }
    let cancelled = require_mutation(
        call(
            first,
            AutomationCall::ScheduleCancel(ScheduleEnableRequest {
                operation_id: operation()?,
                schedule_id: changed.schedule_id,
            }),
        )
        .await?,
    )?;
    if cancelled.definition.enabled || cancelled.next_due_at.is_some() {
        return Err("cancelled schedule retained a deadline".into());
    }
    Ok(original)
}

pub(super) fn require_schedule(answer: AutomationAnswer) -> ConformanceResult<ScheduleSnapshot> {
    match answer {
        AutomationAnswer::Schedule(snapshot) => Ok(snapshot),
        other => Err(format!("expected schedule: {other:?}").into()),
    }
}
pub(super) fn require_mutation(answer: AutomationAnswer) -> ConformanceResult<ScheduleSnapshot> {
    match answer {
        AutomationAnswer::Mutation {
            automation: collaboration_protocol::CatalogAutomation::Schedule(snapshot),
            already_sent,
        } if already_sent.is_empty() => Ok(snapshot),
        other => Err(format!("expected schedule mutation before cutoff: {other:?}").into()),
    }
}
pub(super) fn operation() -> ConformanceResult<collaboration_protocol::OperationId> {
    Ok(durable_steps::Uuid::now_v7().to_string().try_into()?)
}
