use crate::{LocalResult, ObjectActors, session_runtime::SessionRuntime};
use axum::{
    Extension, Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
pub struct LocalSettings {
    pub root: PathBuf,
    pub access_policy: AccessPolicy,
    pub jwks_url: String,
}
#[derive(Clone)]
pub(crate) struct HttpState {
    pub(crate) runtime: SessionRuntime,
    pub(crate) access: crate::access_verification::AccessVerification,
}
pub struct LocalServer {
    base_url: String,
    runtime: SessionRuntime,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}
pub fn credential_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
pub(crate) fn bearer_hash(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(credential_hash)
}
impl LocalServer {
    pub async fn start(settings: LocalSettings) -> LocalResult<Self> {
        Self::start_on(settings, std::net::SocketAddr::from(([127, 0, 0, 1], 0))).await
    }
    pub async fn start_on(
        settings: LocalSettings,
        listen: std::net::SocketAddr,
    ) -> LocalResult<Self> {
        let runtime = SessionRuntime::new(ObjectActors::new(&settings.root)?)?;
        runtime.restore_alarms().await?;
        let state = HttpState {
            runtime: runtime.clone(),
            access: crate::access_verification::AccessVerification::new(
                settings.access_policy,
                settings.jwks_url,
            )?,
        };
        let router = Router::new()
            .route("/v1/session", get(upgrade))
            .route("/admin/v1/machines/enroll", post(enroll))
            .route("/admin/v1/import/projects/{project}", post(import_project))
            .route("/admin/v1/import/fleet", post(import_fleet))
            .route("/admin/v1/copies/{project}/health", get(copy_health))
            .route("/admin/v1/copies/{project}/repair", post(copy_repair))
            .route("/admin/v1/machines/{installation}/revoke", post(revoke))
            .route("/admin/v1/projects/{project}/move", post(move_project))
            .route(
                "/admin/v1/projects/{project}/takeover/preview",
                post(preview),
            )
            .route("/admin/v1/projects/{project}/takeover", post(takeover));
        #[cfg(feature = "conformance")]
        let router = router.route(
            "/__conformance/control",
            post(crate::conformance_control::control),
        );
        let router = router
            .fallback(|| async { (StatusCode::UNAUTHORIZED, Json(AccessRefusal::RouteRefused)) })
            .layer(middleware::from_fn_with_state(state.clone(), access_gate))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(listen).await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            base_url,
            runtime,
            stop: Some(stop),
            task,
        })
    }
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    #[cfg(feature = "conformance")]
    pub fn runtime(&self) -> &SessionRuntime {
        &self.runtime
    }
    pub async fn shutdown(mut self) -> LocalResult<()> {
        self.runtime.stop.cancel();
        self.runtime.tasks.close();
        self.runtime.sockets.lock().await.clear();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.task.await??;
        self.runtime.tasks.wait().await;
        self.runtime.cancel_alarms().await;
        self.runtime.actors.shutdown().await?;
        Ok(())
    }
}
async fn enroll(
    State(state): State<HttpState>,
    Extension(caller): Extension<AccessCaller>,
    Json(request): Json<AdminRequest>,
) -> Response {
    if !matches!(request, AdminRequest::EnrollMachine { .. }) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    admin(state, caller, request).await
}
async fn revoke(
    State(state): State<HttpState>,
    Path(installation): Path<InstallationId>,
    Extension(caller): Extension<AccessCaller>,
) -> Response {
    admin(state, caller, AdminRequest::RevokeMachine { installation }).await
}
async fn copy_health(
    State(state): State<HttpState>,
    Path(project): Path<ProjectId>,
    Extension(caller): Extension<AccessCaller>,
) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(reason) => return (StatusCode::UNAUTHORIZED, Json(reason)).into_response(),
    };
    run_owner(
        &state.runtime,
        &owner,
        OwnerEdgeEffect::CopyOps(project, CopyOpsCall::Health {}),
    )
    .await
}
async fn copy_repair(
    State(state): State<HttpState>,
    Path(project): Path<ProjectId>,
    Extension(caller): Extension<AccessCaller>,
) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(reason) => return (StatusCode::UNAUTHORIZED, Json(reason)).into_response(),
    };
    run_owner(
        &state.runtime,
        &owner,
        OwnerEdgeEffect::CopyOps(project, CopyOpsCall::Repair {}),
    )
    .await
}
async fn preview(
    State(state): State<HttpState>,
    Path(project): Path<ProjectId>,
    Extension(caller): Extension<AccessCaller>,
) -> Response {
    admin(state, caller, AdminRequest::TakeoverPreview { project }).await
}
async fn move_project(
    State(state): State<HttpState>,
    Path(project): Path<ProjectId>,
    Extension(caller): Extension<AccessCaller>,
    Json(request): Json<AdminRequest>,
) -> Response {
    if !matches!(&request,AdminRequest::Move{project:body_project,..} if body_project==&project) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    admin(state, caller, request).await
}
async fn takeover(
    State(state): State<HttpState>,
    Path(project): Path<ProjectId>,
    Extension(caller): Extension<AccessCaller>,
    Json(request): Json<AdminRequest>,
) -> Response {
    if !matches!(&request,AdminRequest::Takeover{project:body_project,..}if body_project==&project)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    admin(state, caller, request).await
}
async fn admin(state: HttpState, caller: AccessCaller, request: AdminRequest) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(reason) => return (StatusCode::UNAUTHORIZED, Json(reason)).into_response(),
    };
    run_owner(&state.runtime, &owner, owner.registry(request)).await
}
async fn run_owner(
    runtime: &SessionRuntime,
    owner: &OwnerIdentity,
    effect: OwnerEdgeEffect,
) -> Response {
    let result: LocalResult<Response> = async {
        Ok(match effect {
            OwnerEdgeEffect::Registry(call) => match runtime
                .invoke_edge::<RegistryKind>((), call.into_call())
                .await?
            {
                RegistryResult::Admin(AdminResult::Retired {})
                | RegistryResult::Admin(AdminResult::ProjectImport(
                    collaboration_replication_protocol::ProjectImportAnswer::Retired,
                )) => (StatusCode::GONE, Json(CopyAdminFailure::Retired {})).into_response(),
                RegistryResult::Admin(AdminResult::Unavailable { reason, .. }) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(CopyAdminFailure::from_reason(reason)),
                )
                    .into_response(),
                RegistryResult::Admin(result) => Json(result).into_response(),
                _ => return Err("invalid admin result".into()),
            },
            OwnerEdgeEffect::CopyOps(project, call) => match runtime
                .invoke_edge::<CopyKind>(project, CopyCall::Ops(call))
                .await?
            {
                CopyResult::Health(health) => Json(health).into_response(),
                CopyResult::Migrated(result) => Json(result).into_response(),
                CopyResult::Repaired => Json(CopyResult::Repaired).into_response(),
                CopyResult::Retired => {
                    (StatusCode::GONE, Json(CopyAdminFailure::Retired {})).into_response()
                }
                CopyResult::Unavailable(reason) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(CopyAdminFailure::from_reason(reason)),
                )
                    .into_response(),
                _ => return Err("invalid copy operations result".into()),
            },
            OwnerEdgeEffect::CopyImport(project, call) => {
                let result = runtime
                    .invoke_edge::<CopyKind>(
                        project.clone(),
                        CopyCall::ImportSeed {
                            project,
                            upload: call.upload,
                        },
                    )
                    .await?;
                let imported = match result {
                    CopyResult::ImportSeed(imported) => imported,
                    CopyResult::Unavailable(reason) => {
                        return Ok((
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(CopyAdminFailure::from_reason(reason)),
                        )
                            .into_response());
                    }
                    _ => return Err("invalid import result".into()),
                };
                if matches!(
                    imported.answer,
                    collaboration_replication_protocol::ProjectImportAnswer::Retired
                ) {
                    return Ok(
                        (StatusCode::GONE, Json(CopyAdminFailure::Retired {})).into_response()
                    );
                }
                if let Some(unheld) = imported.unheld
                    && runtime
                        .invoke_edge::<RegistryKind>((), owner.record_unheld(unheld).into_call())
                        .await?
                        != RegistryResult::UnheldRecorded
                {
                    return Err("invalid unheld result".into());
                }
                Json(imported.answer).into_response()
            }
            OwnerEdgeEffect::FleetImport(call) => {
                let digest = call
                    .bundle
                    .digest()
                    .map_err(|error| CentralError::Encoding {
                        message: error.to_string(),
                    })?;
                eprintln!("fleet import bundle digest={}", digest.as_str());
                match runtime
                    .invoke_edge::<FleetKind>(
                        (),
                        FleetCall::Import {
                            bundle: call.bundle,
                        },
                    )
                    .await?
                {
                    FleetResult::Imported(result) => Json(result).into_response(),
                    _ => return Err("invalid fleet import result".into()),
                }
            }
        })
    }
    .await;
    result.unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn upgrade(
    State(state): State<HttpState>,
    Extension(caller): Extension<AccessCaller>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(hash) = bearer_hash(&headers) else {
        return (StatusCode::UNAUTHORIZED, Json(CredentialRejection::Unknown)).into_response();
    };
    let AccessCaller::Machine { client_id } = caller else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(CredentialRejection::NotMachine),
        )
            .into_response();
    };
    let credential = match state
        .runtime
        .invoke_edge::<RegistryKind>(
            (),
            RegistryCall::AuthenticateMachine {
                credential_hash: hash,
                access_client_id: client_id,
            },
        )
        .await
    {
        Ok(RegistryResult::Authenticated(credential @ CredentialStatus::Machine { .. })) => {
            credential
        }
        Ok(RegistryResult::Authenticated(CredentialStatus::Rejected(reason))) => {
            return (StatusCode::UNAUTHORIZED, Json(reason)).into_response();
        }
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let machine = match EnrolledInstallation::from_credential(credential) {
        Ok(machine) => machine,
        Err(reason) => return (StatusCode::UNAUTHORIZED, Json(reason)).into_response(),
    };
    let MachineEdgeEffect::OpenSession(_installation) = machine.effect();
    let credential = machine.credential();
    upgrade
        .on_upgrade(move |socket| async move {
            let runtime = state.runtime;
            let socket_runtime = runtime.clone();
            let task = runtime
                .tasks
                .spawn(serve_socket(socket_runtime, credential, socket));
            let _ = task.await;
        })
        .into_response()
}
async fn access_gate(
    State(state): State<HttpState>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    #[cfg(feature = "conformance")]
    let path = if path == "/__conformance/control" {
        "/admin/v1/conformance"
    } else {
        path
    };
    match state
        .access
        .verify(
            request
                .headers()
                .get("Cf-Access-Jwt-Assertion")
                .and_then(|value| value.to_str().ok()),
            path,
            state.runtime.now(),
        )
        .await
    {
        Ok(caller) => {
            request.extensions_mut().insert(caller);
            next.run(request).await
        }
        Err(reason) => (StatusCode::UNAUTHORIZED, Json(reason)).into_response(),
    }
}
async fn serve_socket(
    runtime: SessionRuntime,
    credential: CredentialStatus,
    mut socket: WebSocket,
) {
    let CredentialStatus::Machine { installation, .. } = &credential else {
        return;
    };
    let address = ObjectAddress::Connection(installation.clone());
    let Ok(connection) = ConnectionId::new(uuid::Uuid::now_v7().to_string()) else {
        return;
    };
    let (writer, mut outbound) = mpsc::unbounded_channel();
    let (failed, mut failures) = mpsc::unbounded_channel();
    runtime
        .sockets
        .lock()
        .await
        .insert(connection.as_str().into(), writer);
    let mut requests = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _=runtime.stop.cancelled()=>break,
            _=failures.recv()=>break,
            Some(_)=requests.join_next(),if !requests.is_empty()=>{},
            frame=outbound.recv()=>{
                let Some(frame)=frame else{break;};
                let closed=matches!(frame,CentralFrame::Closed{..});
                let Ok(bytes)=serde_json::to_vec(&frame) else{break;};
                if socket.send(Message::Binary(bytes.into())).await.is_err()||closed{break;}
            },
            frame=socket.recv()=>{
                let Some(Ok(frame))=frame else{break;};
                let bytes=match frame{
                    Message::Binary(bytes)=>bytes.to_vec(),Message::Text(text)=>text.as_bytes().to_vec(),
                    Message::Close(_)=>break,Message::Ping(_)|Message::Pong(_)=>continue,
                };
                let input=match decode_machine_frame(&bytes){
                    Err(failure)=>WireInput::Session(SessionCall::DecodeFailure{connection:connection.clone(),failure}),
                    Ok(MachineFrame::Hello{installation,session_id,schema_support})=>WireInput::Session(SessionCall::Hello{connection:connection.clone(),credential:credential.clone(),installation,session_id,schema_support}),
                    Ok(MachineFrame::ForwardAnswer{forward_id,reply})=>WireInput::Session(SessionCall::ForwardAnswer{connection:connection.clone(),forward_id,reply}),
                    Ok(MachineFrame::Request{frame_id,body})=>WireInput::Session(SessionCall::Frame{connection:connection.clone(),frame_id,request:body}),
                };
                let effects=match runtime.actors.step(address.clone(),input,runtime.now()).await{Ok(effects)=>effects,Err(_)=>break};
                let task_runtime=runtime.clone();let task_address=address.clone();let failure=failed.clone();
                requests.spawn(async move{if task_runtime.carry(task_address,effects).await.is_err(){let _=failure.send(());}});
            }
        }
    }
    requests.abort_all();
    while requests.join_next().await.is_some() {}
    runtime.sockets.lock().await.remove(connection.as_str());
    let _ = runtime
        .drive(
            address,
            WireInput::Session(SessionCall::Disconnected { connection }),
        )
        .await;
}

async fn import_project(
    State(state): State<HttpState>,
    Path(project): Path<ProjectId>,
    Extension(caller): Extension<AccessCaller>,
    Json(upload): Json<ProjectImportPage>,
) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(reason) => return (StatusCode::UNAUTHORIZED, Json(reason)).into_response(),
    };
    run_owner(
        &state.runtime,
        &owner,
        OwnerEdgeEffect::CopyImport(
            project,
            CopyImportCall {
                upload: Box::new(upload),
            },
        ),
    )
    .await
}

async fn import_fleet(
    State(state): State<HttpState>,
    Extension(caller): Extension<AccessCaller>,
    Json(bundle): Json<fleet_admission::FleetImportBundle>,
) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(reason) => return (StatusCode::UNAUTHORIZED, Json(reason)).into_response(),
    };
    run_owner(
        &state.runtime,
        &owner,
        OwnerEdgeEffect::FleetImport(FleetImportCall {
            bundle: Box::new(bundle),
        }),
    )
    .await
}
