//! Modern machine links share the native HTTP listener and retain core admission.
use crate::{TokioWorkspaceHost, TokioWorkspaceStores, object_storage::ObjectStorage};
use axum::{
    Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket, close_code},
    },
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::get,
};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, LinkRequest, ProtocolError, ReplyPayload, RequestPayload,
    TokenRenewalRefusal, TokenRenewalResult, decode_text, encode_text,
};
use collaboration_central_core::{ValidatedMachineConnection, WorkspaceCaller, WorkspaceRuntime};
use fleet_identity_protocol::OrganizationId;
use std::sync::Arc;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use workspace_model::{
    RefusalReason, WorkspaceReply, WorkspaceResult,
    central_mail::{CentralMailRefusal, CentralMailResult},
};

type Runtime = WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>;

struct LinkState {
    host: Arc<TokioWorkspaceHost>,
    stop: CancellationToken,
    tasks: TaskTracker,
}

/// The listener owner cancels links before draining HTTP, then waits before closing stores.
pub struct WorkspaceLink {
    state: Arc<LinkState>,
}

impl WorkspaceLink {
    pub fn new(host: Arc<TokioWorkspaceHost>) -> Self {
        Self {
            state: Arc::new(LinkState {
                host,
                stop: CancellationToken::new(),
                tasks: TaskTracker::new(),
            }),
        }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/v1/organizations/{organization}/link", get(upgrade_link))
            .with_state(self.state.clone())
    }

    pub fn begin_shutdown(&self) {
        self.state.stop.cancel();
    }

    /// Call after the listener has drained HTTP admission handlers.
    pub async fn shutdown(&self) {
        self.begin_shutdown();
        self.state.tasks.close();
        self.state.tasks.wait().await;
    }
}

impl Drop for WorkspaceLink {
    fn drop(&mut self) {
        self.begin_shutdown();
    }
}

async fn upgrade_link(
    State(state): State<Arc<LinkState>>,
    Path(organization): Path<String>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    // Track admission as well as the upgraded connection, including failures.
    let task = state.tasks.token();
    if state.stop.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Some(token) = bearer_token(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(organization) = OrganizationId::try_from(organization) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let runtime = match state.host.central(&organization).await {
        Ok(runtime) => runtime,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let connection = match runtime.admit_machine_connection(token).await {
        Ok(Ok(connection)) if connection.organization_id() == &organization => connection,
        Ok(_) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let Some(central_time) =
        chrono::DateTime::from_timestamp_millis(state.host.current_time().unix_millis())
    else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if state.stop.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    upgrade
        .on_upgrade(move |socket| async move {
            let _task = task;
            serve_link(socket, state, runtime, connection, central_time).await;
        })
        .into_response()
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer")
        || token.is_empty()
        || token.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return None;
    }
    Some(token)
}

async fn serve_link(
    mut socket: WebSocket,
    state: Arc<LinkState>,
    runtime: Arc<Runtime>,
    mut connection: ValidatedMachineConnection,
    central_time: workspace_model::Timestamp,
) {
    let welcome = LinkFrame::Data(DataFrame::Welcome {
        machine_id: connection.installation_machine_id().clone(),
        central_time,
    });
    // Keep Welcome queued locally until its safety attempt finishes, so observing it is a barrier.
    if let Err(failure) = runtime.recompute_alarm().await {
        tracing::warn!(failure = ?failure, "post_welcome_alarm_recompute_failed");
    }
    if !send_frame(&mut socket, &state.stop, &welcome).await {
        return;
    }
    loop {
        let message = tokio::select! {
            biased;
            () = state.stop.cancelled() => return,
            message = socket.recv() => message,
        };
        let Some(Ok(message)) = message else {
            return;
        };
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => return,
            // axum handles RFC WebSocket control frames; application heartbeats
            // are the protocol's literal text ping and pong.
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Binary(_) => {
                close_link(
                    &mut socket,
                    &state.stop,
                    close_code::UNSUPPORTED,
                    "text frames required",
                )
                .await;
                return;
            }
        };
        let frame = match decode_text(&text, FrameDirection::ToCentral) {
            Ok(frame) => frame,
            Err(ProtocolError::WrongDirection) => {
                close_link(
                    &mut socket,
                    &state.stop,
                    close_code::UNSUPPORTED,
                    "unsupported link direction",
                )
                .await;
                return;
            }
            Err(_) => {
                close_link(
                    &mut socket,
                    &state.stop,
                    close_code::PROTOCOL,
                    "invalid link frame",
                )
                .await;
                return;
            }
        };
        match frame {
            LinkFrame::Ping => {
                if !send_frame(&mut socket, &state.stop, &LinkFrame::Pong).await {
                    return;
                }
            }
            LinkFrame::Data(DataFrame::Request { request }) => {
                match reply_to_request(&mut socket, &state.stop, &runtime, &connection, &request)
                    .await
                {
                    RequestOutcome::Replied => {}
                    RequestOutcome::AuthorityLapsed => {
                        close_lapsed_link(&mut socket, &state.stop).await;
                        return;
                    }
                    RequestOutcome::Stopped => return,
                }
            }
            LinkFrame::Data(DataFrame::Batch { requests }) => {
                // Every member is answered, as on the Worker; a lapsed authority
                // refuses each member and closes the link after the batch.
                let mut lapsed = false;
                for request in requests.into_requests() {
                    match reply_to_request(
                        &mut socket,
                        &state.stop,
                        &runtime,
                        &connection,
                        &request,
                    )
                    .await
                    {
                        RequestOutcome::Replied => {}
                        RequestOutcome::AuthorityLapsed => lapsed = true,
                        RequestOutcome::Stopped => return,
                    }
                }
                if lapsed {
                    close_lapsed_link(&mut socket, &state.stop).await;
                    return;
                }
            }
            LinkFrame::Data(DataFrame::RenewMachineToken {
                correlation_id,
                token,
            }) => {
                let result = renew(&runtime, &mut connection, token.expose()).await;
                let answer = LinkFrame::Data(DataFrame::MachineTokenRenewal {
                    correlation_id,
                    result,
                });
                if !send_frame(&mut socket, &state.stop, &answer).await {
                    return;
                }
            }
            // Acknowledgements and delivery belong to the delivery owner (C4).
            // Direction validation also rejects server-only frames here.
            _ => {
                close_link(
                    &mut socket,
                    &state.stop,
                    close_code::UNSUPPORTED,
                    "unsupported link operation",
                )
                .await;
                return;
            }
        }
    }
}

/// R40/V32 renewal on the open connection: a renewed connection replaces the
/// current one; on refusal the current connection and its expiry stand.
async fn renew(
    runtime: &Runtime,
    connection: &mut ValidatedMachineConnection,
    token: &str,
) -> TokenRenewalResult {
    use collaboration_central_core::MachineTokenRenewalRefusal as Refusal;
    let refused = |reason| TokenRenewalResult::Refused { reason };
    match runtime.renew_machine_connection(connection, token).await {
        Ok(Ok(renewed)) => {
            let Some(effective_expires_at) =
                chrono::DateTime::from_timestamp(renewed.effective_expires_at().0, 0)
            else {
                return refused(TokenRenewalRefusal::CentralUnavailable);
            };
            *connection = renewed;
            TokenRenewalResult::Renewed {
                effective_expires_at,
            }
        }
        Ok(Err(refusal)) => refused(match refusal {
            Refusal::TokenRejected => TokenRenewalRefusal::TokenRejected,
            Refusal::EnrollmentNotActive => TokenRenewalRefusal::EnrollmentNotActive,
            Refusal::DifferentMachine => TokenRenewalRefusal::DifferentMachine,
            Refusal::DifferentEnrollment => TokenRenewalRefusal::DifferentEnrollment,
        }),
        Err(_) => refused(TokenRenewalRefusal::CentralUnavailable),
    }
}

/// What became of one request frame on the link.
enum RequestOutcome {
    Replied,
    /// The connection's authority expired or was revoked: the request was
    /// answered Refused(MachineRevoked) and the link must close.
    AuthorityLapsed,
    /// The link stopped or failed; it is already closed or closing.
    Stopped,
}

/// Closes a link whose authority lapsed. A renewal past the deadline is
/// refused, so the Router's only way on is a fresh connection; the Worker
/// closes with the same code and reason.
async fn close_lapsed_link(socket: &mut WebSocket, stop: &CancellationToken) {
    close_link(
        socket,
        stop,
        close_code::POLICY,
        "machine authority expired or revoked",
    )
    .await;
}

async fn reply_to_request(
    socket: &mut WebSocket,
    stop: &CancellationToken,
    runtime: &Runtime,
    connection: &ValidatedMachineConnection,
    request: &LinkRequest,
) -> RequestOutcome {
    if stop.is_cancelled() {
        return RequestOutcome::Stopped;
    }
    // Each request restores the connection against live rows first, as the
    // Worker does: after the deadline or a revocation it is refused.
    let (result, outcome) = match runtime
        .restore_machine_connection(&connection.attachment())
        .await
    {
        Ok(Ok(_)) => match dispatch_request(runtime, connection, request).await {
            Ok(result) => (result, RequestOutcome::Replied),
            Err(_) => {
                close_link(socket, stop, close_code::ERROR, "central unavailable").await;
                return RequestOutcome::Stopped;
            }
        },
        Ok(Err(_)) => (
            refused_reply(request.payload(), RefusalReason::MachineRevoked),
            RequestOutcome::AuthorityLapsed,
        ),
        Err(_) => {
            close_link(socket, stop, close_code::ERROR, "central unavailable").await;
            return RequestOutcome::Stopped;
        }
    };
    let sent = send_frame(
        socket,
        stop,
        &LinkFrame::Data(DataFrame::Reply {
            correlation_id: request.correlation_id().clone(),
            result: Box::new(result),
        }),
    )
    .await;
    if sent {
        outcome
    } else {
        RequestOutcome::Stopped
    }
}

fn refused_reply(payload: &RequestPayload, reason: RefusalReason) -> ReplyPayload {
    match payload {
        RequestPayload::Workspace(_) => ReplyPayload::Workspace(WorkspaceReply {
            result: WorkspaceResult::Refused(reason),
            actors: vec![],
        }),
        RequestPayload::Mail(_) => ReplyPayload::Mail(CentralMailResult::Refused(
            CentralMailRefusal::Workspace(reason),
        )),
    }
}

async fn dispatch_request(
    runtime: &Runtime,
    connection: &ValidatedMachineConnection,
    request: &LinkRequest,
) -> Result<ReplyPayload, central_storage::StoreFailure> {
    let caller = WorkspaceCaller::new(connection.clone(), request.caller().clone());
    match request.payload() {
        RequestPayload::Workspace(request) => {
            let reply = match caller {
                Ok(caller) => runtime.request(&caller, request.clone()).await?,
                Err(reason) => WorkspaceReply {
                    result: WorkspaceResult::Refused(reason),
                    actors: vec![],
                },
            };
            Ok(ReplyPayload::Workspace(reply))
        }
        RequestPayload::Mail(request) => {
            let reply = match caller {
                Ok(caller) => runtime.request_central(&caller, request.clone()).await?,
                Err(reason) => CentralMailResult::Refused(CentralMailRefusal::Workspace(reason)),
            };
            Ok(ReplyPayload::Mail(reply))
        }
    }
}

async fn send_frame(socket: &mut WebSocket, stop: &CancellationToken, frame: &LinkFrame) -> bool {
    let Ok(text) = encode_text(frame, FrameDirection::ToRouter) else {
        return false;
    };
    tokio::select! {
        biased;
        () = stop.cancelled() => false,
        result = socket.send(Message::Text(text.into())) => result.is_ok(),
    }
}

async fn close_link(
    socket: &mut WebSocket,
    stop: &CancellationToken,
    code: u16,
    reason: &'static str,
) {
    tokio::select! {
        biased;
        () = stop.cancelled() => {},
        _ = socket.send(Message::Close(Some(CloseFrame { code, reason: reason.into() }))) => {},
    }
}
