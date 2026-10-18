use crate::{
    object_context::ids,
    object_rpc::{self, ObjectRequest, RpcIdentity},
};
use collaboration_central_core::*;
use collaboration_replication_protocol::{CredentialRejection, InstallationId, ProjectId};
use sha2::{Digest, Sha256};
use worker::*;

async fn invoke_edge<To: WireKind>(
    env: &Env,
    to: To::Address,
    caller: RpcIdentity,
    call: To::Call,
) -> Result<To::Result>
where
    EdgeKind: MayCall<To>,
{
    let address = To::address(&to);
    let call_id = ids()?
        .call_ids
        .pop()
        .ok_or_else(|| Error::RustError("call ID missing".into()))?;
    let request = ObjectRequest {
        address: address.clone(),
        caller,
        caller_kind: EdgeKind::KIND,
        #[cfg(feature = "conformance")]
        test_clock: crate::conformance_control::edge_clock(env, address.clone()).await?,
        input: WireInput::Call {
            from: ObjectAddress::Registry,
            call_id,
            call: Box::new(To::call(call)),
        },
        now: RegistryTime(chrono::Utc::now()),
    };
    To::decode_result(
        object_rpc::decode(object_rpc::request(env, &address, "/invoke", &request).await?).await?,
    )
    .map_err(|error| Error::RustError(error.to_string()))
}
/// Serialized conformance input is narrowed to a legal edge before invoking it.
#[cfg(feature = "conformance")]
pub(crate) async fn invoke(
    env: &Env,
    address: ObjectAddress,
    caller: RpcIdentity,
    call: WireCall,
) -> Result<WireResult> {
    match (address, call) {
        (ObjectAddress::Registry, WireCall::Registry(call)) => Ok(RegistryKind::result(
            invoke_edge::<RegistryKind>(env, (), caller, call).await?,
        )),
        (ObjectAddress::Copy(project), WireCall::Copy(call)) => Ok(CopyKind::result(
            invoke_edge::<CopyKind>(env, project, caller, call).await?,
        )),
        (ObjectAddress::Fleet, WireCall::Fleet(call)) => Ok(FleetKind::result(
            invoke_edge::<FleetKind>(env, (), caller, call).await?,
        )),
        (ObjectAddress::Connection(installation), WireCall::Session(call)) => Ok(
            SessionKind::result(invoke_edge::<SessionKind>(env, installation, caller, call).await?),
        ),
        (_, call) => Err(Error::RustError(format!(
            "edge cannot invoke {:?}",
            call.kind()
        ))),
    }
}
pub(crate) async fn fetch(mut request: Request, env: Env) -> Result<Response> {
    let path = request.path();
    let labels = crate::access_verification::InvocationLabels::for_request(&request)?;
    let verification_path = path.as_str();
    #[cfg(feature = "conformance")]
    let verification_path = if path == "/__conformance/control" {
        "/admin/v1/conformance"
    } else {
        verification_path
    };
    let assertion = request.headers().get("Cf-Access-Jwt-Assertion")?;
    let access = match crate::access_verification::verify(
        &env,
        assertion.as_deref(),
        verification_path,
        &labels,
        crate::access_verification::VerificationLayer::Edge,
    )
    .await
    {
        Ok(caller) => caller,
        Err(reason) => return Ok(Response::from_json(&reason)?.with_status(401)),
    };
    let caller = match access {
        AccessCaller::Machine { client_id } => {
            let Some(token) = request
                .headers()
                .get("authorization")?
                .and_then(|value| value.strip_prefix("Bearer ").map(str::to_owned))
            else {
                return rejected(CredentialRejection::Unknown);
            };
            let result = invoke_edge::<RegistryKind>(
                &env,
                (),
                RpcIdentity::Edge,
                RegistryCall::AuthenticateMachine {
                    credential_hash: format!("{:x}", Sha256::digest(token.as_bytes())),
                    access_client_id: client_id,
                },
            )
            .await?;
            let RegistryResult::Authenticated(credential) = result else {
                return Response::error("invalid authentication response", 500);
            };
            match EnrolledInstallation::from_credential(credential) {
                Ok(machine) => VerifiedCaller::Machine(machine),
                Err(reason) => return rejected(reason),
            }
        }
        owner @ AccessCaller::Owner { .. } => VerifiedCaller::Owner(
            OwnerIdentity::from_access(owner)
                .map_err(|error| Error::RustError(error.to_string()))?,
        ),
    };
    match caller {
        VerifiedCaller::Machine(machine) => {
            if path != "/v1/session" {
                return Ok(Response::from_json(&AccessRefusal::WrongCallerClass)?.with_status(401));
            }
            let effect = machine.effect();
            open_session(&request, &env, machine, effect).await
        }
        VerifiedCaller::Owner(owner) => {
            #[cfg(feature = "conformance")]
            if path == "/__conformance/control" {
                return crate::conformance_control::edge_control(request, env, labels).await;
            }
            if !path.starts_with("/admin/v1/") {
                return Ok(Response::from_json(&AccessRefusal::RouteRefused)?.with_status(401));
            }
            let effect = match owner_effect(&mut request, &path, &owner).await? {
                OwnerRoute::Effect(effect) => effect,
                OwnerRoute::Refused(response) => return Ok(response),
            };
            run_owner(&env, &owner, effect).await
        }
    }
}
async fn open_session(
    request: &Request,
    env: &Env,
    machine: EnrolledInstallation,
    effect: MachineEdgeEffect,
) -> Result<Response> {
    let MachineEdgeEffect::OpenSession(installation) = effect;
    if &installation != machine.installation() {
        return rejected(CredentialRejection::NotMachine);
    }
    let headers = Headers::new();
    for name in [
        "upgrade",
        "connection",
        "sec-websocket-key",
        "sec-websocket-version",
        "sec-websocket-protocol",
    ] {
        if let Some(value) = request.headers().get(name)? {
            headers.set(name, &value)?;
        }
    }
    headers.set(
        "x-central-credential",
        &serde_json::to_string(&machine.credential())?,
    )?;
    #[cfg(feature = "conformance")]
    if let Some(clock) =
        crate::conformance_control::edge_clock(env, ObjectAddress::Connection(installation.clone()))
            .await?
    {
        headers.set("x-central-test-clock", &serde_json::to_string(&clock)?)?;
    }
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    object_rpc::stub(env, &SessionKind::address(&installation))?
        .fetch_with_request(Request::new_with_init(
            "https://central.internal/v1/session",
            &init,
        )?)
        .await
}
enum OwnerRoute {
    Effect(OwnerEdgeEffect),
    Refused(Response),
}
async fn owner_effect(
    request: &mut Request,
    path: &str,
    owner: &OwnerIdentity,
) -> Result<OwnerRoute> {
    let pieces = path.split('/').collect::<Vec<_>>();
    if path == "/admin/v1/import/fleet" {
        if request.method() != Method::Post {
            return Ok(OwnerRoute::Refused(Response::error("POST required", 405)?));
        }
        let bundle = serde_json::from_str(&request.text().await?)?;
        return Ok(OwnerRoute::Effect(OwnerEdgeEffect::FleetImport(
            FleetImportCall {
                bundle: Box::new(bundle),
            },
        )));
    }
    if path.starts_with("/admin/v1/import/projects/") {
        if pieces.len() != 6 {
            return Ok(OwnerRoute::Refused(
                Response::from_json(&AccessRefusal::RouteRefused)?.with_status(401),
            ));
        }
        if request.method() != Method::Post {
            return Ok(OwnerRoute::Refused(Response::error("POST required", 405)?));
        }
        let project = ProjectId::new(
            pieces
                .get(5)
                .ok_or_else(|| Error::RustError("import project path".into()))?
                .to_string(),
        )
        .map_err(|error| Error::RustError(error.to_string()))?;
        let upload = serde_json::from_str(&request.text().await?)?;
        return Ok(OwnerRoute::Effect(OwnerEdgeEffect::CopyImport(
            project,
            CopyImportCall {
                upload: Box::new(upload),
            },
        )));
    }
    let project = || -> Result<ProjectId> {
        ProjectId::new(
            pieces
                .get(4)
                .ok_or_else(|| Error::RustError("project path".into()))?
                .to_string(),
        )
        .map_err(|error| Error::RustError(error.to_string()))
    };
    if pieces.len() == 6 && pieces.get(3) == Some(&"copies") && pieces.get(5) == Some(&"health") {
        if request.method() != Method::Get {
            return Ok(OwnerRoute::Refused(Response::error("GET required", 405)?));
        }
        return Ok(OwnerRoute::Effect(OwnerEdgeEffect::CopyOps(
            project()?,
            CopyOpsCall::Health {},
        )));
    }
    if pieces.len() == 6 && pieces.get(3) == Some(&"copies") && pieces.get(5) == Some(&"repair") {
        if request.method() != Method::Post {
            return Ok(OwnerRoute::Refused(Response::error("POST required", 405)?));
        }
        return Ok(OwnerRoute::Effect(OwnerEdgeEffect::CopyOps(
            project()?,
            CopyOpsCall::Repair {},
        )));
    }
    let operation = if path == "/admin/v1/machines/enroll" {
        let operation = serde_json::from_str(&request.text().await?)?;
        if !matches!(operation, AdminRequest::EnrollMachine { .. }) {
            return Ok(OwnerRoute::Refused(Response::error(
                "enrollment body required",
                400,
            )?));
        }
        operation
    } else if path.starts_with("/admin/v1/machines/") && path.ends_with("/revoke") {
        AdminRequest::RevokeMachine {
            installation: InstallationId::new(
                pieces
                    .get(4)
                    .ok_or_else(|| Error::RustError("installation path".into()))?
                    .to_string(),
            )
            .map_err(|error| Error::RustError(error.to_string()))?,
        }
    } else if path.ends_with("/takeover/preview") {
        AdminRequest::TakeoverPreview {
            project: project()?,
        }
    } else if path.ends_with("/move") || path.ends_with("/takeover") {
        let operation = serde_json::from_str(&request.text().await?)?;
        let project = project()?;
        if !matches!(&operation,AdminRequest::Move{project:body,..}|AdminRequest::Takeover{project:body,..} if body==&project)
        {
            return Ok(OwnerRoute::Refused(Response::error(
                "project body mismatch",
                400,
            )?));
        }
        operation
    } else {
        return Ok(OwnerRoute::Refused(
            Response::from_json(&AccessRefusal::RouteRefused)?.with_status(401),
        ));
    };
    Ok(OwnerRoute::Effect(owner.registry(operation)))
}
async fn run_owner(env: &Env, owner: &OwnerIdentity, effect: OwnerEdgeEffect) -> Result<Response> {
    match effect {
        OwnerEdgeEffect::CopyOps(project, call) => {
            match invoke_edge::<CopyKind>(env, project, RpcIdentity::Admin, CopyCall::Ops(call))
                .await?
            {
                CopyResult::Health(health) => Response::from_json(&health),
                CopyResult::Migrated(result) => Response::from_json(&result),
                CopyResult::Repaired => Response::from_json(&CopyResult::Repaired),
                CopyResult::Retired => {
                    Ok(Response::from_json(&CopyAdminFailure::Retired {})?.with_status(410))
                }
                CopyResult::Unavailable(reason) => {
                    Ok(Response::from_json(&CopyAdminFailure::from_reason(reason))?
                        .with_status(503))
                }
                _ => Response::error("invalid copy operations result", 500),
            }
        }
        OwnerEdgeEffect::Registry(call) => {
            match invoke_edge::<RegistryKind>(env, (), RpcIdentity::Admin, call.into_call()).await?
            {
                RegistryResult::Admin(AdminResult::Retired {})
                | RegistryResult::Admin(AdminResult::ProjectImport(
                    collaboration_replication_protocol::ProjectImportAnswer::Retired,
                )) => Ok(Response::from_json(&CopyAdminFailure::Retired {})?.with_status(410)),
                RegistryResult::Admin(AdminResult::Unavailable { reason, .. }) => {
                    Ok(Response::from_json(&CopyAdminFailure::from_reason(reason))?
                        .with_status(503))
                }
                RegistryResult::Admin(result) => Response::from_json(&result),
                _ => Response::error("invalid admin result", 500),
            }
        }
        OwnerEdgeEffect::CopyImport(project, call) => {
            let result = invoke_edge::<CopyKind>(
                env,
                project.clone(),
                RpcIdentity::Admin,
                CopyCall::ImportSeed {
                    project,
                    upload: call.upload,
                },
            )
            .await?;
            let imported = match result {
                CopyResult::ImportSeed(imported) => imported,
                CopyResult::Unavailable(reason) => {
                    return Ok(Response::from_json(&CopyAdminFailure::from_reason(reason))?
                        .with_status(503));
                }
                _ => return Response::error("invalid import result", 500),
            };
            if matches!(
                imported.answer,
                collaboration_replication_protocol::ProjectImportAnswer::Retired
            ) {
                return Ok(Response::from_json(&CopyAdminFailure::Retired {})?.with_status(410));
            }
            if let Some(unheld) = imported.unheld {
                match invoke_edge::<RegistryKind>(
                    env,
                    (),
                    RpcIdentity::Admin,
                    owner.record_unheld(unheld).into_call(),
                )
                .await?
                {
                    RegistryResult::UnheldRecorded => {}
                    _ => return Response::error("invalid unheld result", 500),
                }
            }
            Response::from_json(&imported.answer)
        }
        OwnerEdgeEffect::FleetImport(call) => {
            let digest = call
                .bundle
                .digest()
                .map_err(|error| Error::RustError(error.to_string()))?;
            console_log!("fleet import bundle digest={}", digest.as_str());
            match invoke_edge::<FleetKind>(
                env,
                (),
                RpcIdentity::Admin,
                FleetCall::Import {
                    bundle: call.bundle,
                },
            )
            .await?
            {
                FleetResult::Imported(result) => Response::from_json(&result),
                _ => Response::error("invalid fleet import result", 500),
            }
        }
    }
}
fn rejected(reason: CredentialRejection) -> Result<Response> {
    Ok(Response::from_json(&reason)?.with_status(401))
}
