//! Verified setup decisions share one org-owned directory, ledger and audit batch.
use crate::{
    AdminRefusal, ObjectHost, OrganizationAdminResult, WorkspaceRuntime, WorkspaceStoreCalls,
    organization_admin_contract::WorkspaceView,
    organization_change_rows::{
        self, ChangeRequestId, OrganizationChangeKind, OrganizationChangeOrigin,
        OrganizationChangeRecord,
    },
    person_request::{CreateWorkspaceRequest, VerifiedPersonRequest},
    workspace_decode_error::corrupt,
    workspace_directory_rows::{self, DirectoryWorkspace},
    workspace_row_codec::{json, string},
};
use central_storage::{GuardedStatement, StoreFailure};
use workspace_model::{
    ActorIdentityId, Description, Identity, PersonId, RequestId, ResourceName, Timestamp,
    WorkspaceId, WorkspaceState,
};

pub(crate) struct WorkspaceCreatePlan {
    pub result: OrganizationAdminResult,
    pub statements: Vec<GuardedStatement>,
    pub operation: RequestId,
}

fn refused(reason: AdminRefusal) -> OrganizationAdminResult {
    OrganizationAdminResult::Refused { reason }
}

pub(crate) fn workspace_view(row: DirectoryWorkspace) -> Result<WorkspaceView, StoreFailure> {
    Ok(WorkspaceView {
        workspace_id: row
            .workspace_id
            .to_string()
            .try_into()
            .map_err(|_| corrupt("workspaces.workspace_id"))?,
        name: row
            .name
            .as_str()
            .to_owned()
            .try_into()
            .map_err(|_| corrupt("workspaces.name"))?,
        description: row
            .description
            .as_str()
            .to_owned()
            .try_into()
            .map_err(|_| corrupt("workspaces.description"))?,
        state: match row.state {
            WorkspaceState::Active => message_board::WorkspaceState::Active,
            WorkspaceState::Archived => message_board::WorkspaceState::Archived,
        },
    })
}

fn retained_result(
    encoded: &str,
    request: &CreateWorkspaceRequest,
) -> Result<OrganizationAdminResult, StoreFailure> {
    let value: serde_json::Value = json(encoded, "request_log.answer")?;
    let answer: OrganizationAdminResult =
        serde_json::from_value(value.clone()).map_err(|_| corrupt("request_log.answer"))?;
    match &answer {
        OrganizationAdminResult::Applied { workspace }
            if workspace.workspace_id.as_str() == request.operation_id.as_str()
                && workspace.name.as_str() == request.name.as_str()
                && value
                    .pointer("/workspace/name")
                    .and_then(serde_json::Value::as_str)
                    == Some(workspace.name.as_str())
                && workspace.description.as_str() == request.description
                && workspace.state == message_board::WorkspaceState::Active =>
        {
            Ok(answer)
        }
        OrganizationAdminResult::Refused {
            reason: AdminRefusal::NameInUse,
        } => Ok(answer),
        _ => Err(corrupt("request_log.answer")),
    }
}

impl<H: ObjectHost, S: WorkspaceStoreCalls> WorkspaceRuntime<H, S> {
    /// Caller has already admitted this fresh person proof at `now` under the org turn.
    pub(crate) async fn prepare_workspace_create(
        &self,
        person: &VerifiedPersonRequest,
        request: &CreateWorkspaceRequest,
        now: Timestamp,
    ) -> Result<WorkspaceCreatePlan, StoreFailure> {
        let operation: RequestId = request
            .operation_id
            .as_str()
            .to_owned()
            .try_into()
            .map_err(|_| corrupt("operation_id"))?;
        let identity = Identity::Person {
            person_id: PersonId::try_from(person.person_id.clone())
                .map_err(|_| corrupt("person_id"))?,
        };
        let actor = self.prepare_identity(&identity, now).await?;
        let cutoff = (now - chrono::Duration::hours(24)).to_rfc3339();
        let logged = self
            .host
            .read(crate::workspace_request_log::lookup(&operation, &cutoff))
            .await?;
        if logged.rows.len() > 1 {
            return Err(corrupt("request_log.request_id"));
        }
        if let Some(row) = logged.rows.first() {
            let stored_actor: ActorIdentityId = string(&logged, row, "caller")?
                .parse()
                .map_err(|_| corrupt("request_log.caller"))?;
            let result = if stored_actor != actor.actor
                || string(&logged, row, "content_digest")? != person.intent_digest
            {
                refused(AdminRefusal::Conflict)
            } else {
                retained_result(string(&logged, row, "answer")?, request)?
            };
            // Replay consumes only its new admitted proof. Neither the retained
            // operation answer nor actor mapping is written a second time.
            return Ok(WorkspaceCreatePlan {
                result,
                statements: vec![],
                operation,
            });
        }
        let name: ResourceName = request
            .name
            .as_str()
            .to_owned()
            .try_into()
            .map_err(|_| corrupt("workspace.name"))?;
        let description: Description = request
            .description
            .clone()
            .try_into()
            .map_err(|_| corrupt("workspace.description"))?;
        let mut statements = actor.statements;
        let result = if workspace_directory_rows::read_by_name(self.host.as_ref(), &name)
            .await?
            .is_some()
        {
            refused(AdminRefusal::NameInUse)
        } else {
            let workspace_id: WorkspaceId = request
                .operation_id
                .as_str()
                .to_owned()
                .try_into()
                .map_err(|_| corrupt("workspace_id"))?;
            statements.push(workspace_directory_rows::insert(
                &workspace_id,
                &name,
                &description,
                now,
            )?);
            statements.push(organization_change_rows::insert(
                &OrganizationChangeRecord {
                    id: self.ids.new_id(),
                    request_id: ChangeRequestId::try_from(
                        request.operation_id.as_str().to_owned(),
                    )?,
                    kind: OrganizationChangeKind::WorkspaceCreated,
                    origin: OrganizationChangeOrigin::Person {
                        person_id: person.person_id.clone(),
                        permission_checked: person.operation.route().permission().to_owned(),
                    },
                    target_id: workspace_id.to_string(),
                    created_at_millis: now.timestamp_millis(),
                },
            )?);
            OrganizationAdminResult::Applied {
                workspace: workspace_view(DirectoryWorkspace {
                    workspace_id,
                    name,
                    description,
                    state: WorkspaceState::Active,
                })?,
            }
        };
        statements.extend(crate::workspace_request_log::retain(
            &operation,
            &actor.actor,
            &person.intent_digest,
            &result,
            &now.to_rfc3339(),
            &cutoff,
        )?);
        Ok(WorkspaceCreatePlan {
            result,
            statements,
            operation,
        })
    }
}
