//! Test-only HTTP envelopes feed bearer inputs to actual core admission.
use super::{CentralObject, OrganizationId};
use serde::Deserialize;
use worker::{Request, Response, Result};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthenticatedRequest<Request> {
    token: String,
    caller: workspace_model::Caller,
    request: Request,
}

pub(super) async fn dispatch(
    object: &CentralObject,
    organization: OrganizationId,
    mut request: Request,
) -> Result<Response> {
    let runtime = object.runtime(organization).await?;
    match request.path().as_str() {
        "/test/request" => {
            let input: AuthenticatedRequest<workspace_model::WorkspaceRequest> =
                request.json().await?;
            let result = match runtime.admit_machine_connection(&input.token).await {
                Err(failure) => Err(failure),
                Ok(Ok(connection)) => {
                    match collaboration_central_core::WorkspaceCaller::new(connection, input.caller)
                    {
                        Ok(caller) => runtime.request(&caller, input.request).await,
                        Err(reason) => Ok(workspace_model::WorkspaceReply {
                            result: workspace_model::WorkspaceResult::Refused(reason),
                            actors: vec![],
                        }),
                    }
                }
                Ok(Err(_)) => Ok(workspace_model::WorkspaceReply {
                    result: workspace_model::WorkspaceResult::Refused(
                        workspace_model::RefusalReason::MachineRevoked,
                    ),
                    actors: vec![],
                }),
            };
            Response::from_json(&result)
        }
        "/test/mail" => {
            use workspace_model::central_mail::{CentralMailRefusal, CentralMailResult};
            let input: AuthenticatedRequest<workspace_model::central_mail::CentralMailRequest> =
                request.json().await?;
            let result = match runtime.admit_machine_connection(&input.token).await {
                Err(failure) => Err(failure),
                Ok(Ok(connection)) => {
                    match collaboration_central_core::WorkspaceCaller::new(connection, input.caller)
                    {
                        Ok(caller) => runtime.request_central(&caller, input.request).await,
                        Err(_) => Ok(CentralMailResult::Refused(CentralMailRefusal::Workspace(
                            workspace_model::RefusalReason::MachineRevoked,
                        ))),
                    }
                }
                Ok(Err(_)) => Ok(CentralMailResult::Refused(CentralMailRefusal::Workspace(
                    workspace_model::RefusalReason::MachineRevoked,
                ))),
            };
            Response::from_json(&result)
        }
        _ => Response::error("unknown authenticated fixture request", 404),
    }
}

/// Engine probe routes admit their caller through the same token admission as
/// the link; `None` when the token or the caller is refused.
pub(super) async fn admitted_caller(
    runtime: &super::super::OrgRuntime,
    token: &str,
    caller: workspace_model::Caller,
) -> Result<Option<collaboration_central_core::WorkspaceCaller>> {
    match runtime.admit_machine_connection(token).await {
        Ok(Ok(connection)) => {
            Ok(collaboration_central_core::WorkspaceCaller::new(connection, caller).ok())
        }
        Ok(Err(_)) => Ok(None),
        Err(failure) => Err(worker::Error::RustError(failure.to_string())),
    }
}
