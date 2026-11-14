//! Replay precedes mutable rule decisions; the durable answer shares the effect batch.
use crate::workspace_decode_error::corrupt;
use crate::workspace_row_codec::WorkspaceRows;
use crate::{ObjectHost, WorkspaceCaller, WorkspaceRuntime, WorkspaceStoreCalls};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch, StoreFailure,
};
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
        Err(error @ StoreFailure::Corrupt { .. }) => Err(error),
    }
}
pub(crate) async fn execute<Host: ObjectHost, Stores: WorkspaceStoreCalls>(
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
    let actor = runtime.resolve_identity(&identity).await?;
    let now = runtime.now()?;
    let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
    let digest = crate::workspace_request_log::digest(&request.operation)?;
    if let m::WorkspaceOperation::CreateWorkspace { name } = &request.operation {
        let request_id = request
            .request_id
            .as_ref()
            .ok_or_else(|| corrupt("request_id"))?;
        let _turn = runtime.central_turn.lock().await;
        let logged = runtime
            .host
            .read(crate::workspace_request_log::lookup(request_id, &cutoff))
            .await?;
        if let Some(answer) = crate::workspace_request_log::replay(&logged, &actor, &digest)? {
            return Ok(reply(answer));
        }
        let workspace = m::Workspace {
            workspace_id: runtime.mint()?,
            name: name.clone(),
            state: m::WorkspaceState::Active,
        };
        let answer = m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated {
            workspace: workspace.clone(),
        });
        let mut statements = vec![GuardedStatement {
            statement: Statement {
                sql: "INSERT INTO workspaces (workspace_id,name,state,created_at,updated_at) VALUES (?,?,'active',?,?)",
                args: vec![
                    SqlValue::Text(workspace.workspace_id.to_string()),
                    SqlValue::Text(name.as_str().to_owned()),
                    SqlValue::Text(now.to_rfc3339()),
                    SqlValue::Text(now.to_rfc3339()),
                ],
            },
            expect: RowExpectation::Exactly(1),
        }];
        statements.extend(crate::workspace_request_log::retain(
            request_id,
            &actor,
            &digest,
            &answer,
            &now.to_rfc3339(),
            &cutoff,
        )?);
        let outcome = runtime
            .host
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
    let workspace = request
        .workspace_id
        .as_ref()
        .ok_or_else(|| corrupt("workspace_id"))?;
    let _turn = if request.operation.is_write() {
        Some(runtime.turns.take(workspace).await)
    } else {
        None
    };
    if request.operation.is_write() {
        let request_id = request
            .request_id
            .as_ref()
            .ok_or_else(|| corrupt("request_id"))?;
        let logged = runtime
            .read_workspace(
                workspace,
                crate::workspace_request_log::lookup(request_id, &cutoff),
            )
            .await?;
        if let Some(answer) = crate::workspace_request_log::replay(&logged, &actor, &digest)? {
            return enrich(runtime, answer, true).await;
        }
    }
    let context = workspace_rules::WriteContext {
        workspace_id: workspace.clone(),
        actor: actor.clone(),
        change_id: runtime.mint()?,
        now,
    };
    let rows = WorkspaceRows::decode(
        runtime
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
    #[cfg(feature = "test-support")]
    runtime
        .pauses
        .reached(workspace, crate::PausePoint::AfterReads)
        .await;
    let resolved = runtime.resolve_command(&request.operation, &actor).await?;
    let decision = match &resolved {
        Ok(command) => crate::workspace_rule_dispatch::run_rule(&context, command, &rows),
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
        .extend(crate::workspace_request_log::retain(
            request_id,
            &actor,
            &digest,
            &answer,
            &now.to_rfc3339(),
            &cutoff,
        )?);
    if let Some(result) = store_result(runtime.batch_workspace(workspace, batch).await)? {
        return Ok(reply(result));
    }
    #[cfg(feature = "test-support")]
    runtime
        .pauses
        .reached(workspace, crate::PausePoint::AfterStoreApplied)
        .await;
    #[cfg(feature = "test-support")]
    if runtime.take_lost_reply(request_id) {
        return Ok(reply(m::WorkspaceResult::OutcomeUnknown));
    }
    enrich(runtime, answer, true).await
}
async fn enrich<Host: ObjectHost, Stores: WorkspaceStoreCalls>(
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
        Op::JoinThread { thread_id }
        | Op::SetNotifications { thread_id, .. }
        | Op::AddThreadMember { thread_id, .. } => Ok(m::ItemRef::Thread(thread_id.clone())),
        _ => Err(corrupt("unchanged item")),
    }
}

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub async fn request(
        &self,
        caller: &WorkspaceCaller,
        request: m::WorkspaceRequest,
    ) -> Result<m::WorkspaceReply, StoreFailure> {
        match execute(self, caller, request).await {
            Err(StoreFailure::Unavailable) => Ok(reply(m::WorkspaceResult::CentralUnavailable)),
            Err(StoreFailure::AnswerLost) => Ok(reply(m::WorkspaceResult::CentralUnavailable)),
            result => result,
        }
    }
}
