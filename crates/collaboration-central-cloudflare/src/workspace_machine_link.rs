//! Hibernating machine connections dispatch independent requests through the owning core.
use super::{CentralObject, OrgRuntime, OrganizationId};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, LinkRequest, LinkRequestBatch, ReplyPayload,
    RequestPayload,
};
use central_storage::StoreFailure;
use collaboration_central_core::{MachineConnectionAttachment, WorkspaceCaller};
use runtime_primitives::Clock;
use worker::{
    Request, Response, Result, WebSocket, WebSocketIncomingMessage, WebSocketPair,
    WebSocketRequestResponsePair,
};
use workspace_model::central_mail::{CentralMailRefusal, CentralMailResult};
use workspace_model::{RefusalReason, WorkspaceReply, WorkspaceResult};

enum IncomingRequests {
    Single(Box<LinkRequest>),
    Batch(LinkRequestBatch),
    Renewal {
        correlation_id: central_link_protocol::CorrelationId,
        token: central_link_protocol::MachineTokenText,
    },
}
enum MachineRequestResult {
    Reply(ReplyPayload),
    AuthorityRefused,
    Unavailable,
}

impl CentralObject {
    pub(super) async fn upgrade_machine_link(
        &self,
        organization: OrganizationId,
        request: &Request,
    ) -> Result<Response> {
        if !crate::workspace_link_entry::is_upgrade(request)? {
            return Response::error("websocket upgrade required", 426);
        }
        let Some(token) = crate::workspace_link_entry::bearer(request)? else {
            return Response::error("machine token required", 401);
        };
        let runtime = self.runtime(organization.clone()).await?;
        let connection = match runtime.admit_machine_connection(&token).await {
            Ok(Ok(connection)) if connection.organization_id() == &organization => connection,
            Ok(_) => return Response::error("machine refused", 401),
            Err(_) => return Response::error("machine admission unavailable", 503),
        };
        let Some(central_time) =
            chrono::DateTime::from_timestamp_millis(self.clock.now().unix_millis())
        else {
            return Response::error("clock unavailable", 503);
        };
        let pair = WebSocketPair::new()?;
        pair.server.serialize_attachment(connection.attachment())?;
        let auto_response =
            WebSocketRequestResponsePair::new("ping", "pong").map_err(worker::Error::from)?;
        self.state.set_websocket_auto_response(&auto_response);
        self.state.accept_web_socket(&pair.server);
        // Queue before the safety call; only send after its attempt has completed.
        let welcome = DataFrame::Welcome {
            machine_id: connection.installation_machine_id().clone(),
            central_time,
        };
        if let Err(failure) = runtime.recompute_alarm().await {
            worker::console_warn!("post_welcome_alarm_recompute_failed failure={failure:?}");
        }
        send(&pair.server, welcome)?;
        Response::from_websocket(pair.client)
    }

    pub(super) async fn handle_machine_message(
        &self,
        socket: WebSocket,
        incoming: WebSocketIncomingMessage,
    ) -> Result<()> {
        let WebSocketIncomingMessage::String(text) = incoming else {
            return close(&socket, "text frame required");
        };
        let frame = match central_link_protocol::decode_text(&text, FrameDirection::ToCentral) {
            Ok(frame) => frame,
            Err(_) => return close(&socket, "invalid link frame"),
        };
        let requests = match frame {
            LinkFrame::Data(DataFrame::Request { request }) => IncomingRequests::Single(request),
            LinkFrame::Data(DataFrame::Batch { requests }) => IncomingRequests::Batch(requests),
            LinkFrame::Data(DataFrame::RenewMachineToken {
                correlation_id,
                token,
            }) => IncomingRequests::Renewal {
                correlation_id,
                token,
            },
            // Literal ping/pong is answered by the platform without calling this handler.
            LinkFrame::Ping
            | LinkFrame::Pong
            | LinkFrame::Data(
                DataFrame::Welcome { .. }
                | DataFrame::Reply { .. }
                | DataFrame::Deliver { .. }
                | DataFrame::Ack { .. }
                | DataFrame::MachineTokenRenewal { .. },
            ) => return close(&socket, "unsupported link event"),
        };
        let attachment = match socket.deserialize_attachment::<MachineConnectionAttachment>() {
            Ok(Some(attachment)) => attachment,
            _ => return close(&socket, "connection identity missing"),
        };
        let organization = attachment.organization_id().clone();
        if self.check_physical_organization(&organization).is_err() {
            return close(&socket, "connection organization mismatch");
        }
        #[cfg(feature = "test-support")]
        super::signin_launcher_controls::sample_clock(self, &organization).await?;
        let runtime = self.runtime(organization).await?;
        match requests {
            IncomingRequests::Renewal {
                correlation_id,
                token,
            } => {
                let result = renew(&socket, runtime.as_ref(), &attachment, token.expose()).await?;
                send(
                    &socket,
                    DataFrame::MachineTokenRenewal {
                        correlation_id,
                        result,
                    },
                )
            }
            IncomingRequests::Single(request) => {
                match self
                    .dispatch_machine_request(runtime.as_ref(), &attachment, &request)
                    .await?
                {
                    MachineRequestResult::Reply(reply) => {
                        self.reply_to_machine(&socket, &request, reply).await
                    }
                    MachineRequestResult::AuthorityRefused => {
                        close(&socket, "machine authority expired or revoked")
                    }
                    MachineRequestResult::Unavailable => {
                        close(&socket, "machine admission unavailable")
                    }
                }
            }
            IncomingRequests::Batch(requests) => {
                for request in requests.requests() {
                    // Every member restores live provenance and enters its own applying turn.
                    // Semantic refusals are replies, so they cannot skip later members.
                    let reply = match self
                        .dispatch_machine_request(runtime.as_ref(), &attachment, request)
                        .await?
                    {
                        MachineRequestResult::Reply(reply) => reply,
                        MachineRequestResult::AuthorityRefused => {
                            refused_reply(request.payload(), RefusalReason::MachineRevoked)
                        }
                        MachineRequestResult::Unavailable => unavailable_reply(request.payload()),
                    };
                    self.reply_to_machine(&socket, request, reply).await?;
                }
                Ok(())
            }
        }
    }

    async fn dispatch_machine_request(
        &self,
        runtime: &OrgRuntime,
        attachment: &MachineConnectionAttachment,
        request: &LinkRequest,
    ) -> Result<MachineRequestResult> {
        let restored = runtime.restore_machine_connection(attachment).await;
        #[cfg(feature = "test-support")]
        self.witness_restored(
            request.correlation_id(),
            match &restored {
                Ok(Ok(_)) => super::workspace_link_witness::RestoreOutcome::Admitted,
                Ok(Err(_)) => super::workspace_link_witness::RestoreOutcome::Refused,
                Err(_) => super::workspace_link_witness::RestoreOutcome::Unavailable,
            },
        )
        .await?;
        let connection = match restored {
            Ok(Ok(connection)) => connection,
            Ok(Err(_)) => return Ok(MachineRequestResult::AuthorityRefused),
            Err(StoreFailure::Unavailable | StoreFailure::AnswerLost) => {
                return Ok(MachineRequestResult::Unavailable);
            }
            Err(
                error @ (StoreFailure::Corrupt { .. } | StoreFailure::AlarmUnpublishable { .. }),
            ) => {
                return Err(worker::Error::RustError(error.to_string()));
            }
        };
        let caller = match WorkspaceCaller::new(connection, request.caller().clone()) {
            Ok(caller) => caller,
            Err(reason) => {
                return Ok(MachineRequestResult::Reply(refused_reply(
                    request.payload(),
                    reason,
                )));
            }
        };
        let reply = match request.payload() {
            RequestPayload::Workspace(operation) => ReplyPayload::Workspace(
                runtime
                    .request(&caller, operation.clone())
                    .await
                    .map_err(|error| worker::Error::RustError(error.to_string()))?,
            ),
            RequestPayload::Mail(operation) => ReplyPayload::Mail(
                runtime
                    .request_central(&caller, operation.clone())
                    .await
                    .map_err(|error| worker::Error::RustError(error.to_string()))?,
            ),
        };
        Ok(MachineRequestResult::Reply(reply))
    }

    async fn reply_to_machine(
        &self,
        socket: &WebSocket,
        request: &LinkRequest,
        reply: ReplyPayload,
    ) -> Result<()> {
        #[cfg(feature = "test-support")]
        self.witness_alarm(request.correlation_id()).await?;
        send(
            socket,
            DataFrame::Reply {
                correlation_id: request.correlation_id().clone(),
                result: Box::new(reply),
            },
        )
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
fn unavailable_reply(payload: &RequestPayload) -> ReplyPayload {
    match payload {
        RequestPayload::Workspace(_) => ReplyPayload::Workspace(WorkspaceReply {
            result: WorkspaceResult::CentralUnavailable,
            actors: vec![],
        }),
        RequestPayload::Mail(_) => ReplyPayload::Mail(CentralMailResult::CentralUnavailable),
    }
}
fn send(socket: &WebSocket, frame: DataFrame) -> Result<()> {
    let text =
        central_link_protocol::encode_text(&LinkFrame::Data(frame), FrameDirection::ToRouter)
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
    socket.send_with_str(text)
}
fn close(socket: &WebSocket, reason: &str) -> Result<()> {
    socket.close(Some(1008), Some(reason))
}

/// R40/V32 renewal on the open hibernating socket. A renewed connection's
/// attachment replaces the old one, so a restored socket carries the renewed
/// expiry; on refusal the old attachment and its expiry stand.
async fn renew(
    socket: &WebSocket,
    runtime: &OrgRuntime,
    attachment: &MachineConnectionAttachment,
    token: &str,
) -> Result<central_link_protocol::TokenRenewalResult> {
    use central_link_protocol::{TokenRenewalRefusal, TokenRenewalResult};
    use collaboration_central_core::MachineTokenRenewalRefusal as Refusal;
    let refused = |reason| TokenRenewalResult::Refused { reason };
    // The current connection is restored against live rows first; a socket
    // whose authority already lapsed cannot renew.
    let current = match runtime.restore_machine_connection(attachment).await {
        Ok(Ok(current)) => current,
        Ok(Err(_)) => return Ok(refused(TokenRenewalRefusal::EnrollmentNotActive)),
        Err(_) => return Ok(refused(TokenRenewalRefusal::CentralUnavailable)),
    };
    Ok(
        match runtime.renew_machine_connection(&current, token).await {
            Ok(Ok(renewed)) => {
                let Some(effective_expires_at) =
                    chrono::DateTime::from_timestamp(renewed.effective_expires_at().0, 0)
                else {
                    return Ok(refused(TokenRenewalRefusal::CentralUnavailable));
                };
                socket.serialize_attachment(renewed.attachment())?;
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
        },
    )
}
