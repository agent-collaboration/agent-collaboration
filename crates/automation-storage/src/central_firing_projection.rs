//! Read-only definitions satisfy existing foreign keys; execution inputs come from the frozen entry.
use crate::{CentralFiringRequest, CentralLocalWork, StorageError};
use agent_automation::{
    CapturedRunInputs, ContinuityInput, FrozenExecutionConfiguration, RunId, ScheduleId,
};
use collaboration_protocol::{EndpointRef, FiringOutcome, FiringWork, SessionRef};
use sqlx::SqliteConnection;

pub(crate) async fn admit_work(
    connection: &mut SqliteConnection,
    request: &CentralFiringRequest,
) -> Result<(CentralLocalWork, FiringOutcome), StorageError> {
    match &request.firing.work {
        FiringWork::Schedule { definition } => {
            admit_central_run(connection, request, definition).await
        }
        FiringWork::Wake { .. } => {
            crate::central_wake_admission::admit_wake(connection, request).await
        }
    }
}
async fn admit_central_run(
    connection: &mut SqliteConnection,
    request: &CentralFiringRequest,
    definition: &collaboration_protocol::ScheduleDefinition,
) -> Result<(CentralLocalWork, FiringOutcome), StorageError> {
    let schedule_id: ScheduleId = request
        .firing
        .automation_id
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    crate::central_definition_projection::refuse_local_projection(
        connection,
        "schedule",
        schedule_id.as_str(),
    )
    .await?;
    let domain_definition = agent_automation::ScheduleDefinition::<SessionRef, EndpointRef> {
        instruction_id: definition.instruction_id.clone(),
        timing: crate::central_definition_projection::project_timing(&definition.timing)?,
        enabled: definition.enabled,
        destination: match &definition.destination {
            collaboration_protocol::ExecutionDestination::OwnedThread { target, cwd } => {
                agent_automation::ExecutionDestination::OwnedThread {
                    target: target.clone(),
                    cwd: cwd.clone(),
                }
            }
            collaboration_protocol::ExecutionDestination::FreshEachRun { endpoint, cwd } => {
                agent_automation::ExecutionDestination::FreshEachRun {
                    endpoint: endpoint.clone(),
                    cwd: cwd.clone(),
                }
            }
            collaboration_protocol::ExecutionDestination::Unprepared
            | collaboration_protocol::ExecutionDestination::FreshEachRunUnprepared => {
                return Err(StorageError::InvalidRecord);
            }
        },
        execution_timeout_seconds: definition.execution_timeout_seconds.map(u32::from),
        model: definition.model.clone(),
        effort: definition.effort.clone(),
    };
    crate::schedule_repository::validate_definition(&domain_definition)?;
    let inventory = crate::run_inventory::load(connection, &schedule_id).await?;
    if let Some(run_id) = inventory.occupying {
        return Err(StorageError::RunInvariantConflict {
            run_ids: vec![run_id],
        });
    }
    let require_summary = matches!(
        domain_definition.destination,
        agent_automation::ExecutionDestination::FreshEachRun { .. }
    );
    let continuity = crate::run_admission::select_continuity(
        connection,
        &schedule_id,
        ContinuityInput::None,
        require_summary,
    )
    .await?;
    let inputs = CapturedRunInputs {
        schedule_change_id: request.firing.revision.clone(),
        instruction_revision_id: request.firing.content.revision_id.clone(),
        instruction_text: request.content.text.clone(),
        continuity,
        execution_configuration: FrozenExecutionConfiguration {
            destination: domain_definition.destination.clone(),
            execution_timeout_seconds: domain_definition.execution_timeout_seconds,
            model: domain_definition.model.clone(),
            effort: domain_definition.effort.clone(),
        },
    };
    let text = request.content.text.as_str();
    let instruction_id = definition.instruction_id.as_str();
    let revision = request.firing.content.revision_id.as_str();
    sqlx::query!("INSERT INTO instruction_documents(instruction_id,current_revision_id,instruction_text,updated_at_ms) VALUES (?,?,?,?) ON CONFLICT(instruction_id) DO NOTHING",instruction_id,revision,text,request.now_ms).execute(&mut *connection).await?;
    sqlx::query!("INSERT INTO instruction_revisions(revision_id,instruction_id,instruction_text,recorded_at_ms) VALUES (?,?,?,?) ON CONFLICT(revision_id) DO NOTHING",revision,instruction_id,text,request.now_ms).execute(&mut *connection).await?;
    let encoded =
        serde_json::to_string(&domain_definition).map_err(|_| StorageError::InvalidRecord)?;
    sqlx::query!("INSERT INTO schedule_definitions(schedule_id,change_id,instruction_id,enabled,definition_json,imported_continuity_json,created_at_ms,updated_at_ms,authority) VALUES (?,?,?,?,?,'{\"kind\":\"none\"}',?,?,'central') ON CONFLICT(schedule_id) DO UPDATE SET change_id=excluded.change_id,instruction_id=excluded.instruction_id,enabled=excluded.enabled,definition_json=excluded.definition_json,updated_at_ms=excluded.updated_at_ms",
        schedule_id.as_str(),request.firing.revision.as_str(),instruction_id,definition.enabled,encoded,request.now_ms,request.now_ms).execute(&mut *connection).await?;
    sqlx::query!("INSERT INTO schedule_timing_state(schedule_id,applied_change_id,anchor_at_ms,evaluated_through_ms,next_due_at_ms) VALUES (?,?,?,?,NULL) ON CONFLICT(schedule_id) DO UPDATE SET applied_change_id=excluded.applied_change_id,next_due_at_ms=NULL",schedule_id.as_str(),request.firing.revision.as_str(),request.now_ms,request.now_ms).execute(&mut *connection).await?;
    let run_id = RunId::generate();
    let due = request.firing.due_at.unix_millis();
    let captured = serde_json::to_string(&inputs).map_err(|_| StorageError::InvalidRecord)?;
    let evidence = serde_json::to_string(&agent_automation::RunExecutionEvidence::<(), (), ()> {
        route: None,
        timing: None,
        acceptance: None,
    })
    .map_err(|_| StorageError::InvalidRecord)?;
    sqlx::query!("INSERT INTO workflow_runs(run_id,schedule_id,due_at_ms,run_status,captured_inputs_json,execution_evidence_json) VALUES (?,?,?,'preparing',?,?)",run_id.as_str(),schedule_id.as_str(),due,captured,evidence).execute(connection).await?;
    Ok((
        CentralLocalWork::Schedule { run_id },
        FiringOutcome::Received,
    ))
}
