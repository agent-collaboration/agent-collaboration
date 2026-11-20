//! The organization's workspace directory over the machine request path:
//! create, rename, archive, show and list. The directory is central's own
//! `workspaces` table, not a workspace store.
use crate::WorkspaceCaller;
use crate::WorkspaceRuntime;
use crate::workspace_request_gate::{reply, store_result};
use central_storage::{AlarmUpdate, StatementBatch, StoreFailure};
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_model as m;
use workspace_rules::DirectoryChange;

pub(crate) enum DirectoryRequest<'a> {
    Create {
        name: &'a m::ResourceName,
    },
    Change {
        workspace_id: &'a m::WorkspaceId,
        change: DirectoryChange,
    },
    Show {
        workspace_id: &'a m::WorkspaceId,
    },
    List {
        query: &'a m::ListQuery,
        cursor: m::ListCursor,
        limit: m::PageLimit,
    },
}

/// Directory operations, with the workspace id each must or must not carry.
/// `Ok(None)` means a workspace-store request; `Err` names the invalid field.
pub(crate) fn classify<'a>(
    request: &'a m::WorkspaceRequest,
) -> Result<Option<DirectoryRequest<'a>>, &'static str> {
    use m::WorkspaceOperation as Op;
    let workspace = request.workspace_id.as_ref();
    let required = || workspace.ok_or("workspaceId");
    let absent = || match workspace {
        Some(_) => Err("workspaceId"),
        None => Ok(()),
    };
    Ok(Some(match &request.operation {
        Op::CreateWorkspace { name } => {
            absent()?;
            DirectoryRequest::Create { name }
        }
        Op::UpdateWorkspace { name, description } => DirectoryRequest::Change {
            workspace_id: required()?,
            change: DirectoryChange::Update {
                name: name.clone(),
                description: description.clone(),
            },
        },
        Op::ArchiveWorkspace => DirectoryRequest::Change {
            workspace_id: required()?,
            change: DirectoryChange::Archive,
        },
        Op::Show {
            item: m::ItemRef::Workspace(item),
        } => {
            let workspace_id = required()?;
            if item != workspace_id {
                return Err("item");
            }
            DirectoryRequest::Show { workspace_id }
        }
        Op::List {
            query: query @ (m::ListQuery::Workspaces { .. } | m::ListQuery::SearchWorkspaces { .. }),
            cursor,
            limit,
        } => {
            absent()?;
            DirectoryRequest::List {
                query,
                cursor: *cursor,
                limit: *limit,
            }
        }
        _ => return Ok(None),
    }))
}

pub(crate) async fn execute<Host: CentralStore, Stores: WorkspaceStores>(
    runtime: &WorkspaceRuntime<Host, Stores>,
    caller: &WorkspaceCaller,
    request: &m::WorkspaceRequest,
    directory: DirectoryRequest<'_>,
) -> Result<m::WorkspaceReply, StoreFailure> {
    match directory {
        DirectoryRequest::Show { workspace_id } => {
            let _central = runtime.execution.take_central_turn().await;
            if let Err(refusal) = runtime
                .access()
                .admit_workspace(
                    runtime,
                    caller.caller(),
                    caller.connection().attachment_ref(),
                    Some(workspace_id),
                )
                .await?
            {
                return Ok(reply(m::WorkspaceResult::Refused(refusal)));
            }
            let workspace = crate::workspace_directory_rows::read_by_id(
                runtime.execution.statement_host(),
                workspace_id,
            )
            .await?
            .ok_or_else(|| corrupt("directory_workspaces.workspace_id"))?;
            Ok(reply(m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
                item: m::ItemView::Workspace(workspace.into()),
            })))
        }
        DirectoryRequest::List {
            query,
            cursor,
            limit,
        } => {
            let _central = runtime.execution.take_central_turn().await;
            if let Err(refusal) = runtime
                .access()
                .admit_workspace(
                    runtime,
                    caller.caller(),
                    caller.connection().attachment_ref(),
                    None,
                )
                .await?
            {
                return Ok(reply(m::WorkspaceResult::Refused(refusal)));
            }
            let workspaces: Vec<m::Workspace> =
                crate::workspace_directory_rows::read_all(runtime.execution.statement_host())
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect();
            let allowed = runtime
                .access()
                .allowed_workspaces(caller.connection().machine_id())
                .await?;
            Ok(reply(
                match workspace_rules::workspace_list_answer(
                    query,
                    cursor,
                    limit,
                    &workspaces,
                    &allowed,
                ) {
                    Ok(answer) => m::WorkspaceResult::Done(answer),
                    Err(refusal) => m::WorkspaceResult::Refused(refusal),
                },
            ))
        }
        DirectoryRequest::Create { .. } | DirectoryRequest::Change { .. } => {
            write(runtime, caller, request, directory).await
        }
    }
}

async fn write<Host: CentralStore, Stores: WorkspaceStores>(
    runtime: &WorkspaceRuntime<Host, Stores>,
    caller: &WorkspaceCaller,
    request: &m::WorkspaceRequest,
    directory: DirectoryRequest<'_>,
) -> Result<m::WorkspaceReply, StoreFailure> {
    let identity = match caller.caller() {
        m::Caller::Session(session) => m::Identity::AgentSession {
            session: session.clone(),
        },
        m::Caller::Person { person_id, .. } => m::Identity::Person {
            person_id: person_id.clone(),
        },
    };
    let request_id = request
        .request_id
        .as_ref()
        .ok_or_else(|| corrupt("request_id"))?;
    // Machine admission under the central turn first; a workspace's own state
    // is checked after replay, so a retried archive replays its answer instead
    // of being refused.
    let _turn = runtime.execution.take_central_turn().await;
    if let Err(refusal) = runtime
        .access()
        .admit_workspace(
            runtime,
            caller.caller(),
            caller.connection().attachment_ref(),
            None,
        )
        .await?
    {
        return Ok(reply(m::WorkspaceResult::Refused(refusal)));
    }
    let now = runtime.execution.now()?;
    let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
    let digest = match &directory {
        DirectoryRequest::Change { workspace_id, .. } => {
            // Directory writes share the organization log across workspace targets.
            fleet_runtime::workspace_request_log::digest(&(workspace_id, &request.operation))?
        }
        _ => fleet_runtime::workspace_request_log::digest(&request.operation)?,
    };
    let actor_plan = runtime.access().prepare_identity(&identity, now).await?;
    let actor = actor_plan.actor;
    let logged = runtime
        .execution
        .statement_host()
        .read(fleet_runtime::workspace_request_log::lookup(
            fleet_runtime::workspace_request_log::RequestLogStore::Organization,
            request_id,
            &cutoff,
        ))
        .await?;
    if let Some(answer) = fleet_runtime::workspace_request_log::replay(&logged, &actor, &digest)? {
        return Ok(reply(answer));
    }
    let mut statements = actor_plan.statements;
    let answer = match directory {
        DirectoryRequest::Create { name } => {
            if crate::workspace_directory_rows::read_by_name(
                runtime.execution.statement_host(),
                name,
            )
            .await?
            .is_some()
            {
                m::WorkspaceResult::Refused(m::RefusalReason::WorkspaceNameInUse {
                    name: name.clone(),
                })
            } else {
                let workspace = m::Workspace {
                    workspace_id: runtime.execution.mint()?,
                    name: name.clone(),
                    description: m::Description::try_from(String::new())
                        .map_err(|_| corrupt("directory_workspaces.description"))?,
                    state: m::WorkspaceState::Active,
                };
                statements.push(crate::workspace_directory_rows::insert(
                    &workspace.workspace_id,
                    name,
                    &workspace.description,
                    now,
                )?);
                m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace })
            }
        }
        DirectoryRequest::Change {
            workspace_id,
            change,
        } => {
            if let Err(refusal) = runtime
                .access()
                .admit_workspace(
                    runtime,
                    caller.caller(),
                    caller.connection().attachment_ref(),
                    Some(workspace_id),
                )
                .await?
            {
                return Ok(reply(m::WorkspaceResult::Refused(refusal)));
            }
            let current: m::Workspace = crate::workspace_directory_rows::read_by_id(
                runtime.execution.statement_host(),
                workspace_id,
            )
            .await?
            .ok_or_else(|| corrupt("directory_workspaces.workspace_id"))?
            .into();
            let holder = match &change {
                DirectoryChange::Update { name, .. } => {
                    crate::workspace_directory_rows::read_by_name(
                        runtime.execution.statement_host(),
                        name,
                    )
                    .await?
                    .map(|row| row.workspace_id)
                }
                DirectoryChange::Archive => None,
            };
            match workspace_rules::change_workspace(now, &current, &change, holder.as_ref()) {
                Ok((statement, workspace)) => {
                    statements.push(statement);
                    m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceUpdated { workspace })
                }
                Err(refusal) => m::WorkspaceResult::Refused(refusal),
            }
        }
        DirectoryRequest::Show { .. } | DirectoryRequest::List { .. } => {
            return Err(corrupt("directory write"));
        }
    };
    statements.extend(fleet_runtime::workspace_request_log::retain(
        fleet_runtime::workspace_request_log::RequestLogStore::Organization,
        request_id,
        &actor,
        &digest,
        &answer,
        &now.to_rfc3339(),
        &cutoff,
    )?);
    let outcome = runtime
        .execution
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
    if runtime.execution.take_lost_reply(request_id) {
        return Ok(reply(m::WorkspaceResult::OutcomeUnknown));
    }
    Ok(reply(answer))
}
