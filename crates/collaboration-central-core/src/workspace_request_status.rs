//! Read a retained decision from the ledger that owned the original write.
use crate::{
    WorkspaceCaller, WorkspaceRuntime,
    organization_admin_contract::OrganizationAdminResult,
    workspace_decode_error::corrupt,
    workspace_request_log::{RequestLogStore, lookup},
    workspace_row_codec::{json, string},
};
use central_link_protocol::{
    ReplyPayload, RequestRefusal, RequestScope, RequestStatusAnswer, RequestStatusQuery,
};
use central_storage::{ReadOutcome, StoreFailure};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
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

fn retained_decision(
    rows: &ReadOutcome,
    caller: &ActorIdentityId,
    scope: &RequestScope,
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
    let answer = string(rows, row, "answer")?;
    match scope {
        RequestScope::Organization => json(answer, "runtime_request_log.answer"),
        RequestScope::Workspace(_) => {
            json(answer, "workspace_request_log.answer").map(OrganizationDecision::Workspace)
        }
    }
    .map(Some)
}

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    /// Status never re-applies the original operation or creates an identity.
    pub async fn request_status(
        &self,
        caller: &WorkspaceCaller,
        query: &RequestStatusQuery,
    ) -> Result<RequestStatusAnswer, StoreFailure> {
        let _turn = self.take_central_turn().await;
        let scope_workspace = match &query.scope {
            RequestScope::Organization => None,
            RequestScope::Workspace(workspace) => Some(workspace),
        };
        if self
            .admit_workspace(caller, scope_workspace)
            .await?
            .is_err()
        {
            return Ok(RequestStatusAnswer::NotFound);
        }
        let Some(actor) = self.existing_caller_actor(caller.caller()).await? else {
            return Ok(RequestStatusAnswer::NotFound);
        };
        let cutoff = (self.now()? - chrono::Duration::hours(24)).to_rfc3339();
        let rows = if let Some(workspace) = scope_workspace {
            self.read_workspace(
                workspace,
                lookup(RequestLogStore::Workspace, &query.request_id, &cutoff),
            )
            .await?
        } else {
            self.host
                .read(lookup(
                    RequestLogStore::Organization,
                    &query.request_id,
                    &cutoff,
                ))
                .await?
        };
        Ok(match retained_decision(&rows, &actor, &query.scope)? {
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
