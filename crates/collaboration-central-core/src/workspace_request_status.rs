//! Read a retained decision from the ledger that owned the original write.
use crate::WorkspaceCaller;
use crate::{WorkspaceRuntime, organization_admin_contract::OrganizationAdminResult};
use central_link_protocol::{
    ReplyPayload, RequestRefusal, RequestScope, RequestStatusAnswer, RequestStatusQuery,
};
use central_storage::{ReadOutcome, StoreFailure};
use fleet_contracts::{
    ReadWorkspaceRequestStatus, RetainedWorkspaceStatus, WorkspaceRequest, WorkspaceServiceRpc,
};
use fleet_rpc::{RpcClient, Transport};
use fleet_runtime::workspace_request_log::{RequestLogStore, lookup};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_runtime_ports::{
    stored_values::{json, string},
    workspace_decode_error::corrupt,
};
use workspace_model::{
    ActorIdentityId, WorkspaceReply, WorkspaceResult, central_mail::CentralMailResult,
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
        Stores: Transport<WorkspaceRequest>,
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
            let client = RpcClient::<WorkspaceRequest, _>::new(
                workspace.clone(),
                self.execution.workspace_stores(),
            );
            match client
                .read_request_status(ReadWorkspaceRequestStatus {
                    request_id: query.request_id.clone(),
                    actor,
                    cutoff,
                })
                .await?
            {
                RetainedWorkspaceStatus::NotFound => None,
                RetainedWorkspaceStatus::Refused(reason) => Some(OrganizationDecision::Workspace(
                    WorkspaceResult::Refused(reason),
                )),
                RetainedWorkspaceStatus::Answered(result) => {
                    Some(OrganizationDecision::Workspace(*result))
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
                OrganizationDecision::Workspace(_)
                | OrganizationDecision::Mail(_)
                | OrganizationDecision::PrivateAdmin(_),
            ) => {
                return Err(corrupt("request log answer"));
            }
        })
    }
}
