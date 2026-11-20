//! Replay precedes mutable rule decisions; the durable answer shares the effect batch.
use crate::workspace_row_codec::WorkspaceRows;
use crate::{WorkspaceCaller, WorkspaceRuntime};
use central_storage::{AlarmUpdate, BatchOutcome, StatementBatch, StoreFailure};
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_model as m;

fn invalid(field: &str) -> m::WorkspaceReply {
    reply(m::WorkspaceResult::Refused(
        m::RefusalReason::InvalidInput {
            field: field.to_owned(),
        },
    ))
}
pub(crate) fn reply(result: m::WorkspaceResult) -> m::WorkspaceReply {
    m::WorkspaceReply {
        result,
        actors: vec![],
    }
}
pub(crate) fn store_result(
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
    if request.operation.is_write() && request.request_id.is_none() {
        return Ok(invalid("requestId"));
    }
    if let m::WorkspaceOperation::List {
        query: m::ListQuery::MessageStatus { message_id },
        ..
    } = &request.operation
    {
        if request.workspace_id.is_some() {
            return Ok(invalid("workspaceId"));
        }
        let result = match runtime.direct_message_status(caller, message_id).await? {
            Ok(status) => m::WorkspaceResult::Done(m::WorkspaceAnswer::MessageStatus { status }),
            Err(refusal) => m::WorkspaceResult::Refused(refusal),
        };
        return Ok(reply(result));
    }
    match crate::workspace_directory_requests::classify(&request) {
        Err(field) => return Ok(invalid(field)),
        Ok(Some(directory)) => {
            return crate::workspace_directory_requests::execute(
                runtime, caller, &request, directory,
            )
            .await;
        }
        Ok(None) => {}
    }
    if request.workspace_id.is_none() {
        return Ok(invalid("workspaceId"));
    }
    {
        let _central = runtime.execution.take_central_turn().await;
        if let Err(refusal) = runtime
            .admit_workspace(caller, request.workspace_id.as_ref())
            .await?
        {
            return Ok(reply(m::WorkspaceResult::Refused(refusal)));
        }
    }
    let identity = match caller.caller() {
        m::Caller::Session(session) => m::Identity::AgentSession {
            session: session.clone(),
        },
        m::Caller::Person { person_id, .. } => m::Identity::Person {
            person_id: person_id.clone(),
        },
    };
    let actor = {
        let _turn = runtime.execution.take_central_turn().await;
        if let Err(refusal) = runtime
            .admit_workspace(caller, request.workspace_id.as_ref())
            .await?
        {
            return Ok(reply(m::WorkspaceResult::Refused(refusal)));
        }
        runtime.resolve_identity(&identity).await?
    };
    let digest = fleet_runtime::workspace_request_log::digest(&request.operation)?;
    let workspace = request
        .workspace_id
        .as_ref()
        .ok_or_else(|| corrupt("workspace_id"))?;
    let _turn = if request.operation.is_write() {
        Some(runtime.execution.workspace_turns().take(workspace).await)
    } else {
        None
    };
    let admission_turn = runtime.execution.take_central_turn().await;
    if let Err(refusal) = runtime.admit_workspace(caller, Some(workspace)).await? {
        return Ok(reply(m::WorkspaceResult::Refused(refusal)));
    }
    let now = runtime.execution.now()?;
    let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
    if request.operation.is_write() {
        let request_id = request
            .request_id
            .as_ref()
            .ok_or_else(|| corrupt("request_id"))?;
        let logged = runtime
            .execution
            .read_workspace(
                workspace,
                fleet_runtime::workspace_request_log::lookup(
                    fleet_runtime::workspace_request_log::RequestLogStore::Workspace,
                    request_id,
                    &cutoff,
                ),
            )
            .await?;
        if let Some(answer) =
            fleet_runtime::workspace_request_log::replay(&logged, &actor, &digest)?
        {
            drop(admission_turn);
            if let Err(error) = runtime.drain_after_request(workspace).await {
                return drain_failure(error);
            }
            return enrich(runtime, answer, true).await;
        }
    }
    let context = workspace_rules::WriteContext {
        workspace_id: workspace.clone(),
        actor: actor.clone(),
        change_id: runtime.execution.mint()?,
        now,
    };
    let rows = WorkspaceRows::decode(
        runtime
            .execution
            .read_workspace(
                workspace,
                crate::workspace_read_queries::read_statement(
                    workspace,
                    &actor,
                    &request.operation,
                )?,
            )
            .await?,
    )?;
    if !request.operation.is_write() {
        return enrich(
            runtime,
            crate::workspace_item_reads::read_answer(&context, &request.operation, &rows)?,
            false,
        )
        .await;
    }
    drop(admission_turn);
    #[cfg(feature = "test-support")]
    runtime
        .execution
        .pause_points()
        .reached(workspace, fleet_runtime::PausePoint::AfterReads)
        .await;
    let resolved = runtime.resolve_command(&request.operation, &actor).await?;
    let decision = match &resolved {
        Ok(command) => crate::workspace_rule_inputs::run_rule(&context, command, &rows),
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
    batch
        .statements
        .extend(fleet_runtime::workspace_request_log::retain(
            fleet_runtime::workspace_request_log::RequestLogStore::Workspace,
            request_id,
            &actor,
            &digest,
            &answer,
            &now.to_rfc3339(),
            &cutoff,
        )?);
    {
        // The workspace turn precedes central, matching the drain's lock order.
        // Revocation and the admitted batch cannot pass each other inside this turn.
        let _central = runtime.execution.take_central_turn().await;
        if let Err(refusal) = runtime.admit_workspace(caller, Some(workspace)).await? {
            return Ok(reply(m::WorkspaceResult::Refused(refusal)));
        }
        if matches!(
            answer,
            m::WorkspaceResult::Done(
                m::WorkspaceAnswer::Created { .. }
                    | m::WorkspaceAnswer::Updated { .. }
                    | m::WorkspaceAnswer::Removed { .. }
            )
        ) {
            runtime.preset_workspace_drain_held(workspace, now).await?;
        }
        if let Some(result) =
            store_result(runtime.execution.batch_workspace(workspace, batch).await)?
        {
            return Ok(reply(result));
        }
    }
    #[cfg(feature = "test-support")]
    runtime
        .execution
        .pause_points()
        .reached(workspace, fleet_runtime::PausePoint::AfterStoreApplied)
        .await;
    #[cfg(feature = "test-support")]
    if runtime.execution.take_lost_reply(request_id) {
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
        .actor_views(&crate::workspace_item_reads::actor_ids(&result))
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
