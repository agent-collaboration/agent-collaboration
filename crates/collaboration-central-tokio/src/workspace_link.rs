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
    RequestRefusal, RequestStatusAnswer, TokenRenewalRefusal, TokenRenewalResult, decode_text,
    encode_text,
};
use collaboration_central_core::{MachineLinkRequestAuthority, WorkspaceRuntime};
use collaboration_central_core::{ValidatedMachineConnection, WorkspaceCaller};
use fleet_contracts::mail_link::{
    MailAckIgnored, MailAckSettlement, WELCOME_PAGE_SIZE, WelcomeDeliveryPage,
    WindowedDeliveryDecision,
};
use fleet_identity_protocol::OrganizationId;
use futures_util::FutureExt;
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
        machine_id: connection.machine_id().clone(),
        central_time,
    });
    // Keep Welcome queued locally until its safety attempt finishes, so observing it is a barrier.
    if let Err(failure) = runtime.recompute_alarm().await {
        tracing::warn!(failure = ?failure, "post_welcome_alarm_recompute_failed");
    }
    let registration = runtime
        .register_active_machine(&connection.attachment(), || {
            state.host.push_registry.register(
                connection.organization_id().clone(),
                connection.machine_id().clone(),
            )
        })
        .await;
    let Ok(Ok(mut registration)) = registration else {
        close_link(
            &mut socket,
            &state.stop,
            close_code::POLICY,
            "machine authority expired or revoked",
        );
        return;
    };
    let writes = LinkWrites {
        stop: &state.stop,
        replaced: &registration.replaced,
    };
    if !writes.send(&mut socket, &welcome).await {
        return;
    }
    if !send_welcome_backlog(&mut socket, &state, &writes, &runtime, &connection).await {
        return;
    }
    loop {
        let message = tokio::select! {
            biased;
            () = state.stop.cancelled() => return,
            () = registration.replaced.cancelled() => {
                writes.close(&mut socket, close_code::NORMAL, "machine link ended");
                return;
            }
            message = socket.recv() => message,
            ready = registration.receiver.recv() => {
                let Some(message_id) = ready else { return; };
                if !offer_windowed_delivery(&mut socket, &state, &writes, &runtime, &connection, &message_id).await {
                    return;
                }
                continue;
            }
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
                writes.close(&mut socket, close_code::UNSUPPORTED, "text frames required");
                return;
            }
        };
        let frame = match decode_text(&text, FrameDirection::ToCentral) {
            Ok(frame) => frame,
            Err(ProtocolError::WrongDirection) => {
                writes.close(
                    &mut socket,
                    close_code::UNSUPPORTED,
                    "unsupported link direction",
                );
                return;
            }
            Err(_) => {
                writes.close(&mut socket, close_code::PROTOCOL, "invalid link frame");
                return;
            }
        };
        match frame {
            LinkFrame::Ping => {
                if !writes.send(&mut socket, &LinkFrame::Pong).await {
                    return;
                }
            }
            LinkFrame::Data(DataFrame::Request { request }) => {
                match reply_to_request(&mut socket, &writes, &runtime, &connection, &request, false)
                    .await
                {
                    RequestOutcome::Replied => {}
                    RequestOutcome::AuthorityLapsed => {
                        writes.close_lapsed(&mut socket);
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
                        &writes,
                        &runtime,
                        &connection,
                        &request,
                        true,
                    )
                    .await
                    {
                        RequestOutcome::Replied => {}
                        RequestOutcome::AuthorityLapsed => lapsed = true,
                        RequestOutcome::Stopped => return,
                    }
                }
                if lapsed {
                    writes.close_lapsed(&mut socket);
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
                if !writes.send(&mut socket, &answer).await {
                    return;
                }
            }
            LinkFrame::Data(DataFrame::RequestStatus {
                correlation_id,
                caller,
                query,
            }) => {
                let active = match runtime
                    .restore_machine_connection(&connection.attachment())
                    .await
                {
                    Ok(Ok(active)) => active,
                    Ok(Err(_)) => {
                        writes.close_lapsed(&mut socket);
                        return;
                    }
                    Err(_) => {
                        writes.close(&mut socket, close_code::ERROR, "central unavailable");
                        return;
                    }
                };
                let result = match WorkspaceCaller::from_machine_link(active, caller) {
                    Ok(caller) => runtime.request_status(&caller, &query).await,
                    Err(reason) => Ok(RequestStatusAnswer::Refused(RequestRefusal::Workspace(
                        reason,
                    ))),
                };
                let Ok(result) = result else {
                    writes.close(&mut socket, close_code::ERROR, "central unavailable");
                    return;
                };
                let reply = LinkFrame::Data(DataFrame::RequestStatusReply {
                    correlation_id,
                    result: Box::new(result),
                });
                if !writes.send(&mut socket, &reply).await {
                    return;
                }
            }
            LinkFrame::Data(DataFrame::Ack { message_id, answer }) => {
                match runtime
                    .acknowledge_delivery(&connection.attachment(), &message_id, answer)
                    .await
                {
                    Ok(MailAckSettlement::Ignored(MailAckIgnored::MachineAuthorityLapsed)) => {
                        writes.close_lapsed(&mut socket);
                        return;
                    }
                    Ok(MailAckSettlement::Ignored(reason @ MailAckIgnored::WrongMachine)) => {
                        tracing::warn!(?reason, "machine_delivery_ack_ignored");
                    }
                    Ok(_) => {}
                    Err(_) => {
                        writes.close(&mut socket, close_code::ERROR, "central unavailable");
                        return;
                    }
                }
            }
            // Direction validation also rejects server-only frames here.
            _ => {
                writes.close(
                    &mut socket,
                    close_code::UNSUPPORTED,
                    "unsupported link operation",
                );
                return;
            }
        }
    }
}

async fn send_welcome_backlog(
    socket: &mut WebSocket,
    state: &LinkState,
    writes: &LinkWrites<'_>,
    runtime: &Runtime,
    connection: &ValidatedMachineConnection,
) -> bool {
    let attachment = connection.attachment();
    let mut cursor = None;
    loop {
        let page = tokio::select! {
            biased;
            () = state.stop.cancelled() => return false,
            () = writes.replaced.cancelled() => return false,
            page = runtime.windowed_welcome_page(&attachment, cursor.as_ref()) => page,
        };
        let Ok(WelcomeDeliveryPage::Ready(rows)) = page else {
            return false;
        };
        cursor = rows
            .last()
            .map(fleet_contracts::mail_link::WindowedDelivery::cursor);
        let count = rows.len();
        for row in rows {
            if !offer_windowed_delivery(
                socket,
                state,
                writes,
                runtime,
                connection,
                &row.message.message_id,
            )
            .await
            {
                return false;
            }
        }
        if count < WELCOME_PAGE_SIZE {
            return true;
        }
    }
}

async fn offer_windowed_delivery(
    socket: &mut WebSocket,
    state: &LinkState,
    writes: &LinkWrites<'_>,
    runtime: &Runtime,
    connection: &ValidatedMachineConnection,
    message_id: &workspace_ids::MessageId,
) -> bool {
    let attachment = connection.attachment();
    let decision = tokio::select! {
        biased;
        () = state.stop.cancelled() => return false,
        () = writes.replaced.cancelled() => return false,
        decision = runtime.prepare_windowed_delivery(&attachment, message_id) => decision,
    };
    let delivery = match decision {
        Ok(WindowedDeliveryDecision::Ready(delivery)) => delivery,
        Ok(WindowedDeliveryDecision::Ineligible | WindowedDeliveryDecision::SenderUnresolved) => {
            return true;
        }
        Ok(WindowedDeliveryDecision::AuthorityLapsed) | Err(_) => return false,
    };
    if state.host.current_time().unix_millis() >= delivery.expires_at.timestamp_millis() {
        return true;
    }
    let frame = LinkFrame::Data(DataFrame::Deliver {
        message: delivery.message,
        sender: delivery.sender,
    });
    writes.send(socket, &frame).await
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

async fn reply_to_request(
    socket: &mut WebSocket,
    writes: &LinkWrites<'_>,
    runtime: &Runtime,
    connection: &ValidatedMachineConnection,
    request: &LinkRequest,
    in_batch: bool,
) -> RequestOutcome {
    if writes.stop.is_cancelled() {
        return RequestOutcome::Stopped;
    }
    // Each request restores the connection against live rows first, as the
    // Worker does: after the deadline or a revocation it is refused.
    let (result, outcome) = match runtime
        .restore_machine_link_request(&connection.attachment())
        .await
    {
        Ok(MachineLinkRequestAuthority::Admitted(active)) => {
            match dispatch_request(runtime, &active, request).await {
                Ok(result) => (result, RequestOutcome::Replied),
                Err(_) => {
                    writes.close(socket, close_code::ERROR, "central unavailable");
                    return RequestOutcome::Stopped;
                }
            }
        }
        Ok(MachineLinkRequestAuthority::Refused) => (
            refused_reply(request.payload(), RefusalReason::MachineRevoked),
            RequestOutcome::AuthorityLapsed,
        ),
        Ok(MachineLinkRequestAuthority::TemporarilyUnavailable) if in_batch => (
            ReplyPayload::central_unavailable(request.payload()),
            RequestOutcome::Replied,
        ),
        Ok(MachineLinkRequestAuthority::TemporarilyUnavailable) | Err(_) => {
            writes.close(socket, close_code::ERROR, "central unavailable");
            return RequestOutcome::Stopped;
        }
    };
    let sent = writes
        .send(
            socket,
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
    let caller = WorkspaceCaller::from_machine_link(connection.clone(), request.caller().clone());
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

struct LinkWrites<'a> {
    stop: &'a CancellationToken,
    replaced: &'a CancellationToken,
}

impl LinkWrites<'_> {
    async fn send(&self, socket: &mut WebSocket, frame: &LinkFrame) -> bool {
        let Ok(text) = encode_text(frame, FrameDirection::ToRouter) else {
            return false;
        };
        tokio::select! {
            biased;
            () = self.stop.cancelled() => false,
            () = self.replaced.cancelled() => false,
            result = socket.send(Message::Text(text.into())) => result.is_ok(),
        }
    }

    fn close_lapsed(&self, socket: &mut WebSocket) {
        self.close(
            socket,
            close_code::POLICY,
            "machine authority expired or revoked",
        );
    }

    fn close(&self, socket: &mut WebSocket, code: u16, reason: &'static str) {
        let (code, reason) = if self.replaced.is_cancelled() {
            (close_code::NORMAL, "machine link ended")
        } else {
            (code, reason)
        };
        close_link(socket, self.stop, code, reason);
    }
}

fn close_link(socket: &mut WebSocket, stop: &CancellationToken, code: u16, reason: &'static str) {
    if !stop.is_cancelled() {
        // An idle peer can see the Close. Never wait on a backpressured transport.
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code,
                reason: reason.into(),
            })))
            .now_or_never();
    }
}
