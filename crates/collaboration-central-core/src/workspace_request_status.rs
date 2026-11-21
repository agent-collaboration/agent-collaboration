//! Read a retained decision from the ledger that owned the original write.
use crate::WorkspaceCaller;
use crate::{WorkspaceRuntime, organization_admin_contract::OrganizationAdminResult};
use central_link_protocol::{
    ReplyPayload, RequestRefusal, RequestScope, RequestStatusAnswer, RequestStatusQuery,
};
use central_storage::{ReadOutcome, StoreFailure};
use fleet_contracts::{
    ReadWorkspaceRequestStatus, RetainedWorkspaceStatus, RetainedWriteAnswer,
    WorkspaceServiceClient, WorkspaceServiceProtocol, WorkspaceServiceRpc, WorkspaceStatusFailure,
};
use fleet_rpc::{CallError, Transport};
use fleet_runtime::workspace_request_log::{RequestLogStore, lookup};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_runtime_ports::{
    stored_values::{json, string},
    workspace_decode_error::corrupt,
};
use workspace_model::{
    ActorIdentityId, WorkspaceAnswer, WorkspaceReply, WorkspaceResult,
    central_mail::CentralMailResult,
};

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum OrganizationDecision {
    Workspace(WorkspaceResult),
    Mail(CentralMailResult),
    PrivateAdmin(OrganizationAdminResult),
}

fn retained_organization_decision(
    rows: &ReadOutcome,
    caller: &ActorIdentityId,
) -> Result<Option<OrganizationDecision>, StoreFailure> {
    let row = match rows.rows.as_slice() {
        [] => return Ok(None),
        [row] => row,
        _ => return Err(corrupt("request log request_id")),
    };
    let stored_actor: ActorIdentityId = string(rows, row, "caller")?
        .parse()
        .map_err(|_| corrupt("request log caller"))?;
    if &stored_actor != caller {
        return Ok(None);
    }
    json(string(rows, row, "answer")?, "runtime_request_log.answer").map(Some)
}

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    /// Status never re-applies the original operation or creates an identity.
    pub async fn request_status(
        &self,
        caller: &WorkspaceCaller,
        query: &RequestStatusQuery,
    ) -> Result<RequestStatusAnswer, StoreFailure>
    where
        Stores: Transport<WorkspaceServiceProtocol>,
    {
        let _turn = self.execution.take_central_turn().await;
        let scope_workspace = match &query.scope {
            RequestScope::Organization => None,
            RequestScope::Workspace(workspace) => Some(workspace),
        };
        if self
            .access()
            .admit_workspace(
                self,
                caller.caller(),
                caller.connection().attachment_ref(),
                scope_workspace,
            )
            .await?
            .is_err()
        {
            return Ok(RequestStatusAnswer::NotFound);
        }
        let Some(actor) = self.access().existing_caller_actor(caller.caller()).await? else {
            return Ok(RequestStatusAnswer::NotFound);
        };
        let cutoff = self.execution.now()? - chrono::Duration::hours(24);
        let decision = if let Some(workspace) = scope_workspace {
            let workspace =
                WorkspaceServiceClient::new(workspace.clone(), self.execution.workspace_stores());
            match workspace
                .read_request_status(ReadWorkspaceRequestStatus {
                    request_id: query.request_id.clone(),
                    actor,
                    cutoff,
                })
                .await
                .map_err(|failure| match failure {
                    CallError::NeverReached
                    | CallError::Service(WorkspaceStatusFailure::Unavailable) => {
                        StoreFailure::Unavailable
                    }
                    CallError::AnswerLost => StoreFailure::AnswerLost,
                    CallError::Service(WorkspaceStatusFailure::Corrupt { reason }) => {
                        StoreFailure::Corrupt { reason }
                    }
                })? {
                RetainedWorkspaceStatus::NotFound => None,
                RetainedWorkspaceStatus::Refused(reason) => Some(OrganizationDecision::Workspace(
                    WorkspaceResult::Refused(reason),
                )),
                RetainedWorkspaceStatus::Done(answer) => Some(OrganizationDecision::Workspace(
                    WorkspaceResult::Done(public_workspace_answer(*answer)),
                )),
                RetainedWorkspaceStatus::ItemNotFound { workspace_id, item } => {
                    Some(OrganizationDecision::Workspace(WorkspaceResult::NotFound {
                        workspace_id,
                        item,
                    }))
                }
            }
        } else {
            let rows = self
                .execution
                .statement_host()
                .read(lookup(
                    RequestLogStore::Organization,
                    &query.request_id,
                    &cutoff.to_rfc3339(),
                ))
                .await?;
            retained_organization_decision(&rows, &actor)?
        };
        Ok(match decision {
            None
            | Some(OrganizationDecision::PrivateAdmin(
                OrganizationAdminResult::Applied { .. }
                | OrganizationAdminResult::EngineRetried {}
                | OrganizationAdminResult::EngineDiscarded {}
                | OrganizationAdminResult::Refused { .. },
            )) => RequestStatusAnswer::NotFound,
            Some(OrganizationDecision::Workspace(WorkspaceResult::Refused(reason))) => {
                RequestStatusAnswer::Refused(RequestRefusal::Workspace(reason))
            }
            Some(OrganizationDecision::Mail(CentralMailResult::Refused(reason))) => {
                RequestStatusAnswer::Refused(RequestRefusal::Mail(reason))
            }
            Some(OrganizationDecision::Workspace(
                result @ (WorkspaceResult::Done(_) | WorkspaceResult::NotFound { .. }),
            )) => {
                let actors = self
                    .access()
                    .actor_views(&crate::workspace_item_reads::actor_ids(&result))
                    .await?;
                RequestStatusAnswer::Answered(ReplyPayload::Workspace(WorkspaceReply {
                    result,
                    actors,
                }))
            }
            Some(OrganizationDecision::Mail(result @ CentralMailResult::Done(_))) => {
                RequestStatusAnswer::Answered(ReplyPayload::Mail(result))
            }
            Some(
                OrganizationDecision::Workspace(
                    WorkspaceResult::Conflict
                    | WorkspaceResult::OutcomeUnknown
                    | WorkspaceResult::CentralUnavailable,
                )
                | OrganizationDecision::Mail(
                    CentralMailResult::Conflict
                    | CentralMailResult::OutcomeUnknown
                    | CentralMailResult::CentralUnavailable,
                )
                | OrganizationDecision::PrivateAdmin(
                    OrganizationAdminResult::Found { .. }
                    | OrganizationAdminResult::Records { .. }
                    | OrganizationAdminResult::EngineHealth { .. }
                    | OrganizationAdminResult::EngineRetired { .. },
                ),
            ) => {
                return Err(corrupt("request log answer"));
            }
        })
    }
}

fn public_workspace_answer(answer: RetainedWriteAnswer) -> WorkspaceAnswer {
    match answer {
        RetainedWriteAnswer::Created { item, position } => {
            WorkspaceAnswer::Created { item, position }
        }
        RetainedWriteAnswer::Updated { item, position } => {
            WorkspaceAnswer::Updated { item, position }
        }
        RetainedWriteAnswer::Removed { item, position } => {
            WorkspaceAnswer::Removed { item, position }
        }
        RetainedWriteAnswer::Marked { scope, through } => {
            WorkspaceAnswer::Marked { scope, through }
        }
        RetainedWriteAnswer::Unchanged { item } => WorkspaceAnswer::Unchanged { item },
    }
}
