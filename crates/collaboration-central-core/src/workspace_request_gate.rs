//! Replay precedes mutable rule decisions; the durable answer shares the effect batch.
use crate::WorkspaceRuntime;
use central_storage::{AlarmUpdate, BatchOutcome, StatementBatch, StoreFailure};
use fleet_contracts::WorkspaceCaller;
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_workspace_access::WorkspaceRows;
use workspace_model as m;

fn reply(result: m::WorkspaceResult) -> m::WorkspaceReply {
    m::WorkspaceReply {
        result,
        actors: vec![],
    }
}
fn store_result(
    result: Result<BatchOutcome, StoreFailure>,
) -> Result<Option<m::WorkspaceResult>, StoreFailure> {
    match result {
        Ok(BatchOutcome::Applied { .. }) => Ok(None),
        Ok(BatchOutcome::GuardFailed { .. } | BatchOutcome::ConstraintViolated { .. }) => {
            Ok(Some(m::WorkspaceResult::Conflict))
        }
        Err(StoreFailure::AnswerLost) => Ok(Some(m::WorkspaceResult::OutcomeUnknown)),
        Err(StoreFailure::Unavailable) => Ok(Some(m::WorkspaceResult::CentralUnavailable)),
        Ok(BatchOutcome::StatementRejected { .. }) => {
            Ok(Some(m::WorkspaceResult::CentralUnavailable))
        }
        Err(error @ (StoreFailure::Corrupt { .. } | StoreFailure::AlarmUnpublishable { .. })) => {
            Err(error)
        }
    }
}
pub(crate) async fn execute<Host: CentralStore, Stores: WorkspaceStores>(
    runtime: &WorkspaceRuntime<Host, Stores>,
    caller: &WorkspaceCaller,
    request: m::WorkspaceRequest,
) -> Result<m::WorkspaceReply, StoreFailure> {
    let invalid_field = if request.operation.is_write() && request.request_id.is_none() {
        Some("requestId")
    } else if matches!(
        request.operation,
        m::WorkspaceOperation::CreateWorkspace { .. }
    ) {
        request.workspace_id.as_ref().map(|_| "workspaceId")
    } else if request.workspace_id.is_none() {
        Some("workspaceId")
    } else {
        None
    };
    if let Some(field) = invalid_field {
        return Ok(reply(m::WorkspaceResult::Refused(
            m::RefusalReason::InvalidInput {
                field: field.to_owned(),
            },
        )));
    }
    if let Err(refusal) = runtime
        .workspace_access()
        .admit_workspace(caller, request.workspace_id.as_ref())
        .await?
    {
        return Ok(reply(m::WorkspaceResult::Refused(refusal)));
    }
    let identity = match &caller.caller {
        m::Caller::Session(session) => m::Identity::AgentSession {
            session: session.clone(),
        },
        m::Caller::Person { person_id, .. } => m::Identity::Person {
            person_id: person_id.clone(),
        },
    };
    if let m::WorkspaceOperation::CreateWorkspace { name } = &request.operation {
        let request_id = request
            .request_id
            .as_ref()
            .ok_or_else(|| corrupt("request_id"))?;
        let _turn = runtime.fleet_runtime.take_central_turn().await;
        let now = runtime.fleet_runtime.now()?;
        let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
        let digest = fleet_runtime::workspace_request_log::digest(&request.operation)?;
        let actor_plan = runtime
            .workspace_access()
            .prepare_identity(&identity, now)
            .await?;
        let actor = actor_plan.actor;
        let logged = runtime
            .statement_host()
            .read(fleet_runtime::workspace_request_log::lookup(
                request_id, &cutoff,
            ))
            .await?;
        if let Some(answer) =
            fleet_runtime::workspace_request_log::replay(&logged, &actor, &digest)?
        {
            return Ok(reply(answer));
        }
        let mut statements = actor_plan.statements;
        let answer = if fleet_workspace_directory::read_by_name(runtime.statement_host(), name)
            .await?
            .is_some()
        {
            m::WorkspaceResult::Refused(m::RefusalReason::WorkspaceNameInUse { name: name.clone() })
        } else {
            let workspace = m::Workspace {
                workspace_id: runtime.fleet_runtime.mint()?,
                name: name.clone(),
                state: m::WorkspaceState::Active,
            };
            let description = m::Description::try_from(String::new())
                .map_err(|_| corrupt("workspaces.description"))?;
            statements.push(fleet_workspace_directory::insert(
                &workspace.workspace_id,
                name,
                &description,
                now,
            )?);
            m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace })
        };
        statements.extend(fleet_runtime::workspace_request_log::retain(
            request_id,
            &actor,
            &digest,
            &answer,
            &now.to_rfc3339(),
            &cutoff,
        )?);
        let outcome = runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements,
                alarm: AlarmUpdate::Unchanged,
            })
            .await;
        if let Some(result) = store_result(outcome)? {
            return Ok(reply(result));
        }
        #[cfg(feature = "test-support")]
        if runtime.take_lost_reply(request_id) {
            return Ok(reply(m::WorkspaceResult::OutcomeUnknown));
        }
        return Ok(reply(answer));
    }
    let actor = {
        let _turn = runtime.fleet_runtime.take_central_turn().await;
        runtime
            .workspace_access()
            .resolve_identity(&identity)
            .await?
    };
    let now = runtime.fleet_runtime.now()?;
    let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
    let digest = fleet_runtime::workspace_request_log::digest(&request.operation)?;
    let workspace = request
        .workspace_id
        .as_ref()
        .ok_or_else(|| corrupt("workspace_id"))?;
    let _turn = if request.operation.is_write() {
        Some(
            runtime
                .fleet_runtime
                .workspace_turns()
                .take(workspace)
                .await,
        )
    } else {
        None
    };
    if request.operation.is_write() {
        let request_id = request
            .request_id
            .as_ref()
            .ok_or_else(|| corrupt("request_id"))?;
        let logged = runtime
            .fleet_runtime
            .read_workspace(
                workspace,
                fleet_runtime::workspace_request_log::lookup(request_id, &cutoff),
            )
            .await?;
        if let Some(answer) =
            fleet_runtime::workspace_request_log::replay(&logged, &actor, &digest)?
        {
            if let Err(error) = runtime.drain_after_request(workspace).await {
                return drain_failure(error);
            }
            return enrich(runtime, answer, true).await;
        }
    }
    let context = workspace_rules::WriteContext {
        workspace_id: workspace.clone(),
        actor: actor.clone(),
        change_id: runtime.fleet_runtime.mint()?,
        now,
    };
    let rows = WorkspaceRows::decode(
        runtime
            .fleet_runtime
            .read_workspace(
                workspace,
                fleet_workspace_requests::read_statement(workspace, &actor, &request.operation)?,
            )
            .await?,
    )?;
    if !request.operation.is_write() {
        return enrich(
            runtime,
            fleet_workspace_requests::read_answer(&context, &request.operation, &rows)?,
            false,
        )
        .await;
    }
    #[cfg(feature = "test-support")]
    runtime
        .fleet_runtime
        .pause_points()
        .reached(workspace, fleet_runtime::PausePoint::AfterReads)
        .await;
    let resolved = fleet_workspace_requests::resolve_command(
        &runtime.fleet_runtime,
        &request.operation,
        &actor,
    )
    .await?;
    let decision = match &resolved {
        Ok(command) => fleet_workspace_requests::run_rule(&context, command, &rows),
        Err(refusal) => Err(refusal.clone()),
    };
    let (mut batch, answer) = match decision {
        Ok(plan) => {
            let position = m::ChangePosition::try_from(rows.high_water.get() + 1)
                .map_err(|_| corrupt("position"))?;
            let answer = match plan.answer {
                m::RuleAnswer::Created { item } => m::WorkspaceAnswer::Created { item, position },
                m::RuleAnswer::Updated { item } => m::WorkspaceAnswer::Updated { item, position },
                m::RuleAnswer::Removed { item } => m::WorkspaceAnswer::Removed { item, position },
                m::RuleAnswer::Marked { scope, through } => {
                    m::WorkspaceAnswer::Marked { scope, through }
                }
                m::RuleAnswer::Unchanged => m::WorkspaceAnswer::Unchanged {
                    item: unchanged_item(&request.operation)?,
                },
            };
            (plan.batch, m::WorkspaceResult::Done(answer))
        }
        Err(m::RefusalReason::NotFound { item }) => (
            StatementBatch {
                statements: vec![],
                alarm: AlarmUpdate::Unchanged,
            },
            m::WorkspaceResult::NotFound {
                workspace_id: workspace.clone(),
                item,
            },
        ),
        Err(refusal) => (
            StatementBatch {
                statements: vec![],
                alarm: AlarmUpdate::Unchanged,
            },
            m::WorkspaceResult::Refused(refusal),
        ),
    };
    let request_id = request
        .request_id
        .as_ref()
        .ok_or_else(|| corrupt("request_id"))?;
    if matches!(
        answer,
        m::WorkspaceResult::Done(
            m::WorkspaceAnswer::Created { .. }
                | m::WorkspaceAnswer::Updated { .. }
                | m::WorkspaceAnswer::Removed { .. }
        )
    ) {
        runtime.preset_workspace_drain(workspace, now).await?;
    }
    batch
        .statements
        .extend(fleet_runtime::workspace_request_log::retain(
            request_id,
            &actor,
            &digest,
            &answer,
            &now.to_rfc3339(),
            &cutoff,
        )?);
    if let Some(result) = store_result(
        runtime
            .fleet_runtime
            .batch_workspace(workspace, batch)
            .await,
    )? {
        return Ok(reply(result));
    }
    #[cfg(feature = "test-support")]
    runtime
        .fleet_runtime
        .pause_points()
        .reached(workspace, fleet_runtime::PausePoint::AfterStoreApplied)
        .await;
    #[cfg(feature = "test-support")]
    if runtime.take_lost_reply(request_id) {
        return Ok(reply(m::WorkspaceResult::OutcomeUnknown));
    }
    if let Err(error) = runtime.drain_after_request(workspace).await {
        return drain_failure(error);
    }
    enrich(runtime, answer, true).await
}
fn drain_failure(error: StoreFailure) -> Result<m::WorkspaceReply, StoreFailure> {
    match error {
        StoreFailure::Unavailable | StoreFailure::AnswerLost => {
            Ok(reply(m::WorkspaceResult::OutcomeUnknown))
        }
        error @ (StoreFailure::Corrupt { .. } | StoreFailure::AlarmUnpublishable { .. }) => {
            Err(error)
        }
    }
}
async fn enrich<Host: CentralStore, Stores: WorkspaceStores>(
    runtime: &WorkspaceRuntime<Host, Stores>,
    result: m::WorkspaceResult,
    durable_answer: bool,
) -> Result<m::WorkspaceReply, StoreFailure> {
    let actors = match runtime
        .workspace_access()
        .actor_views(&fleet_workspace_requests::actor_ids(&result))
        .await
    {
        Ok(actors) => actors,
        Err(StoreFailure::Unavailable | StoreFailure::AnswerLost) => {
            return Ok(reply(if durable_answer {
                m::WorkspaceResult::OutcomeUnknown
            } else {
                m::WorkspaceResult::CentralUnavailable
            }));
        }
        Err(error) => return Err(error),
    };
    Ok(m::WorkspaceReply { result, actors })
}
fn unchanged_item(operation: &m::WorkspaceOperation) -> Result<m::ItemRef, StoreFailure> {
    use m::WorkspaceOperation as Op;
    match operation {
        Op::GrantProjectRole { project_id, .. }
        | Op::ReleaseProjectRole { project_id, .. }
        | Op::SetProjectStatus { project_id, .. } => Ok(m::ItemRef::Project(project_id.clone())),
        Op::GrantDeliverableRole { deliverable_id, .. }
        | Op::ReleaseDeliverableRole { deliverable_id, .. } => {
            Ok(m::ItemRef::Deliverable(deliverable_id.clone()))
        }
        Op::SetIssueStatus { issue_id, .. } => Ok(m::ItemRef::Issue(issue_id.clone())),
        Op::SetInitiativeStatus { initiative_id, .. } => {
            Ok(m::ItemRef::Initiative(initiative_id.clone()))
        }
        Op::JoinThread { thread_id, .. }
        | Op::SetNotifications { thread_id, .. }
        | Op::AddThreadMember { thread_id, .. } => Ok(m::ItemRef::Thread(thread_id.clone())),
        _ => Err(corrupt("unchanged item")),
    }
}

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub async fn request(
        &self,
        caller: &WorkspaceCaller,
        request: m::WorkspaceRequest,
    ) -> Result<m::WorkspaceReply, StoreFailure> {
        // Admission answers for the organization as on main; the due batch that
        // follows checks the stamp once for this event before any unit (P3).
        let result = execute(self, caller, request).await;
        let _ = self.after_event().await;
        match result {
            Err(StoreFailure::Unavailable) => Ok(reply(m::WorkspaceResult::CentralUnavailable)),
            Err(StoreFailure::AnswerLost) => Ok(reply(m::WorkspaceResult::CentralUnavailable)),
            result => result,
        }
    }
}
