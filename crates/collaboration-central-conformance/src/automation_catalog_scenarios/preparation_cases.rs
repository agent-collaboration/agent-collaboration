//! The catalog binds prepared owned threads through authenticated machine sessions.
use super::schedule_cases::{operation, require_mutation, require_schedule};
use super::{call, same_answer};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, AutomationRequestConstraint,
    BoundScheduleThread, ExecutionDestination, ScheduleBindPreparedRequest, ScheduleCreateRequest,
    ScheduleSnapshot, ScheduleUpdateRequest,
};
use serde_json::json;

pub(super) async fn run(
    client: &ConformanceClient,
    creator: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    target_service: &str,
    prepared: &ScheduleSnapshot,
) -> ConformanceResult<()> {
    let mut definition = prepared.definition.clone();
    definition.enabled = false;
    definition.destination = ExecutionDestination::Unprepared;
    let original = require_schedule(
        call(
            creator,
            AutomationCall::ScheduleCreate(ScheduleCreateRequest {
                operation_id: operation()?,
                definition: definition.clone(),
            }),
        )
        .await?,
    )?;
    let request = ScheduleBindPreparedRequest {
        automation_id: original.schedule_id.as_str().to_owned().try_into()?,
        expected_revision: original.change_id.clone(),
        operation_id: operation()?,
        bound_thread: BoundScheduleThread {
            target: serde_json::from_value(json!({
                "endpoint":{"serviceId":target_service,"endpointId":"codex-local"},
                "sessionId":"prepared-on-target"
            }))?,
            cwd: "/prepared/workspace".into(),
        },
    };
    client
        .automation_clock(AutomationClockControl::Advance {
            now: collaboration_central_core::RegistryTime("2026-10-31T10:00:01.123Z".parse()?),
        })
        .await?;
    let answer = bind(target, request.clone()).await?;
    let bound = require_mutation(answer.clone())?;
    let mut expected = original.clone();
    expected.change_id = bound.change_id.clone();
    expected.definition.destination = ExecutionDestination::OwnedThread {
        target: request.bound_thread.target.clone(),
        cwd: request.bound_thread.cwd.clone(),
    };
    if bound.change_id == original.change_id {
        return Err("preparation did not raise the revision".into());
    }
    same_answer(
        AutomationAnswer::Schedule(bound.clone()),
        AutomationAnswer::Schedule(expected),
    )?;
    same_answer(bind(target, request.clone()).await?, answer.clone())?;
    let mut changed_body = request.clone();
    changed_body.bound_thread.cwd = "/changed".into();
    if !matches!(
        bind(target, changed_body).await?,
        AutomationAnswer::Refused(AutomationRefusal::OperationConflict { .. })
    ) {
        return Err("prepared operation accepted different binding bytes".into());
    }
    let mut stale = request.clone();
    stale.operation_id = operation()?;
    if !matches!(bind(target, stale).await?,
        AutomationAnswer::Refused(AutomationRefusal::ScheduleChangeConflict { current_change_id })
        if current_change_id == bound.change_id)
    {
        return Err("prepared binding did not report the moved revision".into());
    }
    let mut replacement = bound.definition.clone();
    replacement.effort = Some("low".into());
    require_mutation(
        call(
            creator,
            AutomationCall::ScheduleUpdate(ScheduleUpdateRequest {
                operation_id: operation()?,
                schedule_id: bound.schedule_id.clone(),
                expected_change_id: bound.change_id.clone(),
                definition: replacement,
            }),
        )
        .await?,
    )?;
    // The committed receipt still wins after later revisions, rather than rerunning the bind.
    same_answer(bind(target, request.clone()).await?, answer)?;

    for destination in [
        ExecutionDestination::FreshEachRunUnprepared,
        prepared.definition.destination.clone(),
    ] {
        definition.destination = destination;
        definition.model = Some("fixture-model".into());
        let wrong_mode = require_schedule(
            call(
                creator,
                AutomationCall::ScheduleCreate(ScheduleCreateRequest {
                    operation_id: operation()?,
                    definition: definition.clone(),
                }),
            )
            .await?,
        )?;
        let mut wrong = request.clone();
        wrong.operation_id = operation()?;
        wrong.automation_id = wrong_mode.schedule_id.as_str().to_owned().try_into()?;
        wrong.expected_revision = wrong_mode.change_id;
        refused_constraint(
            bind(target, wrong).await?,
            AutomationRequestConstraint::UnpreparedDestination,
        )?;
    }
    definition.destination = ExecutionDestination::Unprepared;
    let untouched = require_schedule(
        call(
            creator,
            AutomationCall::ScheduleCreate(ScheduleCreateRequest {
                operation_id: operation()?,
                definition,
            }),
        )
        .await?,
    )?;
    let mut invalid = request;
    invalid.automation_id = untouched.schedule_id.as_str().to_owned().try_into()?;
    invalid.expected_revision = untouched.change_id.clone();
    invalid.operation_id = operation()?;
    if !matches!(
        bind(creator, invalid.clone()).await?,
        AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
    ) {
        return Err("binding accepted a target on another installation".into());
    }
    invalid.operation_id = operation()?;
    invalid.bound_thread.cwd = "relative/path".into();
    refused_constraint(
        bind(target, invalid).await?,
        AutomationRequestConstraint::AbsolutePathRequired,
    )?;
    let shown = call(
        creator,
        AutomationCall::ScheduleShow(collaboration_protocol::ScheduleShowRequest {
            schedule_id: untouched.schedule_id.clone(),
        }),
    )
    .await?;
    same_answer(shown, AutomationAnswer::Schedule(untouched))?;
    println!(
        "SCHEDULE_BIND_PREPARED_RESULT {}",
        json!({
            "revisionRaised":true,"headerPreserved":true,"receiptReplayedAfterRevision":true,
            "wrongModeRefused":true,"targetInstallationRequired":true
        })
    );
    Ok(())
}

async fn bind(
    target: &mut NativeBoardSession,
    request: ScheduleBindPreparedRequest,
) -> ConformanceResult<AutomationAnswer> {
    call(target, AutomationCall::ScheduleBindPrepared(request)).await
}

fn refused_constraint(
    answer: AutomationAnswer,
    expected: AutomationRequestConstraint,
) -> ConformanceResult<()> {
    match answer {
        AutomationAnswer::Refused(AutomationRefusal::InvalidRequest { reason, .. })
            if reason == expected =>
        {
            Ok(())
        }
        other => Err(format!("prepared binding lost {expected:?} refusal: {other:?}").into()),
    }
}
