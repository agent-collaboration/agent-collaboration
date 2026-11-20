//! Hibernating machine connections dispatch independent requests through the owning core.
use super::{CentralObject, OrgRuntime, OrganizationId};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, LinkRequest, LinkRequestBatch, ReplyPayload,
    RequestPayload, RequestRefusal, RequestStatusAnswer, RequestStatusQuery,
};
use collaboration_central_core::WorkspaceCaller;
use collaboration_central_core::{MachineLinkRequestAuthority, MailAckIgnored, MailAckSettlement};
use fleet_contracts::MachineConnectionAttachment;
use fleet_runtime_ports::Clock;
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
    Status {
        correlation_id: central_link_protocol::CorrelationId,
        caller: workspace_model::Caller,
        query: RequestStatusQuery,
    },
    Ack {
        message_id: workspace_ids::MessageId,
        answer: central_link_protocol::DeliveryAnswer,
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
        // Queue before the safety call; only send after its attempt has completed.
        let welcome = DataFrame::Welcome {
            machine_id: connection.machine_id().clone(),
            central_time,
        };
        if let Err(failure) = runtime.recompute_alarm().await {
            worker::console_warn!("post_welcome_alarm_recompute_failed failure={failure:?}");
        }
        let tag = connection.machine_id().as_str();
        let older = match runtime
            .register_active_machine(&connection.attachment(), || {
                let older = self.state.get_websockets_with_tag(tag);
                self.state.accept_websocket_with_tags(&pair.server, &[tag]);
                older
            })
            .await
        {
            Ok(Ok(older)) => older,
            Ok(Err(_)) => return Response::error("machine refused", 401),
            Err(_) => return Response::error("machine admission unavailable", 503),
        };
        send(&pair.server, welcome)?;
        for socket in older {
            let _ = socket.close(Some(1000), Some("machine link replaced"));
        }
        self.send_welcome_mail(&runtime, &pair.server, &connection.attachment())
            .await?;
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
            LinkFrame::Data(DataFrame::RequestStatus {
                correlation_id,
                caller,
                query,
            }) => IncomingRequests::Status {
                correlation_id,
                caller,
                query,
            },
            LinkFrame::Data(DataFrame::Ack { message_id, answer }) => {
                IncomingRequests::Ack { message_id, answer }
            }
            // Literal ping/pong is answered by the platform without calling this handler.
            LinkFrame::Ping
            | LinkFrame::Pong
            | LinkFrame::Data(
                DataFrame::Welcome { .. }
                | DataFrame::Reply { .. }
                | DataFrame::RequestStatusReply { .. }
                | DataFrame::Deliver { .. }
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
        let result = self
            .dispatch_machine_event(&runtime, socket, attachment, requests)
            .await;
        self.flush_ready_mail(&runtime).await;
        result
    }

    async fn dispatch_machine_event(
        &self,
        runtime: &std::sync::Arc<OrgRuntime>,
        socket: WebSocket,
        attachment: MachineConnectionAttachment,
        requests: IncomingRequests,
    ) -> Result<()> {
        match requests {
            IncomingRequests::Ack { message_id, answer } => {
                match runtime
                    .acknowledge_delivery(&attachment, &message_id, answer)
                    .await
                {
                    Ok(MailAckSettlement::Ignored(MailAckIgnored::MachineAuthorityLapsed)) => {
                        close(&socket, "machine authority expired or revoked")
                    }
                    Ok(MailAckSettlement::Ignored(reason @ MailAckIgnored::WrongMachine)) => {
                        worker::console_warn!("machine_delivery_ack_ignored reason={reason:?}");
                        Ok(())
                    }
                    Ok(_) => Ok(()),
                    Err(_) => socket.close(Some(1011), Some("central unavailable")),
                }
            }
            IncomingRequests::Status {
                correlation_id,
                caller,
                query,
            } => {
                let active = match runtime.restore_machine_connection(&attachment).await {
                    Ok(Ok(active)) => active,
                    Ok(Err(_)) => return close(&socket, "machine authority expired or revoked"),
                    Err(_) => return socket.close(Some(1011), Some("central unavailable")),
                };
                let result = match WorkspaceCaller::from_machine_link(active, caller) {
                    Ok(caller) => runtime.request_status(&caller, &query).await,
                    Err(reason) => Ok(RequestStatusAnswer::Refused(RequestRefusal::Workspace(
                        reason,
                    ))),
                };
                let Ok(result) = result else {
                    return socket.close(Some(1011), Some("central unavailable"));
                };
                send(
                    &socket,
                    DataFrame::RequestStatusReply {
                        correlation_id,
                        result: Box::new(result),
                    },
                )
            }
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
                    // The request is answered before the close, as on the native host.
                    MachineRequestResult::AuthorityRefused => {
                        let reply = refused_reply(request.payload(), RefusalReason::MachineRevoked);
                        self.reply_to_machine(&socket, &request, reply).await?;
                        close(&socket, "machine authority expired or revoked")
                    }
                    MachineRequestResult::Unavailable => {
                        close(&socket, "machine admission unavailable")
                    }
                }
            }
            IncomingRequests::Batch(requests) => {
                let mut lapsed = false;
                for request in requests.requests() {
                    // Every member restores live provenance and enters its own applying turn.
                    // Semantic refusals are replies, so they cannot skip later members.
                    let reply = match self
                        .dispatch_machine_request(runtime.as_ref(), &attachment, request)
                        .await?
                    {
                        MachineRequestResult::Reply(reply) => reply,
                        MachineRequestResult::AuthorityRefused => {
                            lapsed = true;
                            refused_reply(request.payload(), RefusalReason::MachineRevoked)
                        }
                        MachineRequestResult::Unavailable => {
                            ReplyPayload::central_unavailable(request.payload())
                        }
                    };
                    self.reply_to_machine(&socket, request, reply).await?;
                }
                // A lapsed authority closes the link once every member is answered.
                if lapsed {
                    return close(&socket, "machine authority expired or revoked");
                }
                Ok(())
            }
        }
    }

    async fn send_welcome_mail(
        &self,
        runtime: &OrgRuntime,
        socket: &WebSocket,
        attachment: &MachineConnectionAttachment,
    ) -> Result<()> {
        use collaboration_central_core::{WELCOME_PAGE_SIZE, WelcomeDeliveryPage};
        let mut cursor = None;
        loop {
            let page = runtime
                .windowed_welcome_page(attachment, cursor.as_ref())
                .await
                .map_err(|error| worker::Error::RustError(error.to_string()))?;
            let WelcomeDeliveryPage::Ready(rows) = page else {
                return close(socket, "machine authority expired or revoked");
            };
            cursor = rows
                .last()
                .map(collaboration_central_core::WindowedDelivery::cursor);
            let count = rows.len();
            for row in rows {
                if !self
                    .offer_windowed_mail(runtime, socket, attachment, &row.message.message_id)
                    .await?
                {
                    return Ok(());
                }
            }
            if count < WELCOME_PAGE_SIZE {
                return Ok(());
            }
        }
    }

    pub(super) async fn flush_ready_mail(&self, runtime: &OrgRuntime) {
        let ready = std::mem::take(
            &mut *self
                .mail_pushes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for message in ready {
            for socket in self
                .state
                .get_websockets_with_tag(message.machine_id.as_str())
            {
                if socket.as_ref().ready_state() != web_sys::WebSocket::OPEN {
                    continue;
                }
                if let Ok(Some(attachment)) =
                    socket.deserialize_attachment::<MachineConnectionAttachment>()
                    && let Err(error) = self
                        .offer_windowed_mail(runtime, &socket, &attachment, &message.message_id)
                        .await
                {
                    worker::console_warn!("central delivery offer failed: {error}");
                }
            }
        }
    }

    async fn offer_windowed_mail(
        &self,
        runtime: &OrgRuntime,
        socket: &WebSocket,
        attachment: &MachineConnectionAttachment,
        message_id: &workspace_ids::MessageId,
    ) -> Result<bool> {
        use collaboration_central_core::WindowedDeliveryDecision;
        let decision = runtime
            .prepare_windowed_delivery(attachment, message_id)
            .await
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
        match decision {
            WindowedDeliveryDecision::Ready(delivery) => {
                if socket.as_ref().ready_state() != web_sys::WebSocket::OPEN {
                    return Ok(false);
                }
                if self.clock.now().unix_millis() < delivery.expires_at.timestamp_millis() {
                    send(
                        socket,
                        DataFrame::Deliver {
                            message: delivery.message,
                            sender: delivery.sender,
                        },
                    )?;
                }
                Ok(true)
            }
            WindowedDeliveryDecision::Ineligible | WindowedDeliveryDecision::SenderUnresolved => {
                Ok(true)
            }
            WindowedDeliveryDecision::AuthorityLapsed => {
                close(socket, "machine authority expired or revoked")?;
                Ok(false)
            }
        }
    }

    async fn dispatch_machine_request(
        &self,
        runtime: &OrgRuntime,
        attachment: &MachineConnectionAttachment,
        request: &LinkRequest,
    ) -> Result<MachineRequestResult> {
        let restored = runtime.restore_machine_link_request(attachment).await;
        #[cfg(feature = "test-support")]
        self.witness_restored(
            request.correlation_id(),
            match &restored {
                Ok(MachineLinkRequestAuthority::Admitted(_)) => {
                    super::workspace_link_witness::RestoreOutcome::Admitted
                }
                Ok(MachineLinkRequestAuthority::Refused) => {
                    super::workspace_link_witness::RestoreOutcome::Refused
                }
                Ok(MachineLinkRequestAuthority::TemporarilyUnavailable) | Err(_) => {
                    super::workspace_link_witness::RestoreOutcome::Unavailable
                }
            },
        )
        .await?;
        let connection = match restored {
            Ok(MachineLinkRequestAuthority::Admitted(connection)) => connection,
            Ok(MachineLinkRequestAuthority::Refused) => {
                return Ok(MachineRequestResult::AuthorityRefused);
            }
            Ok(MachineLinkRequestAuthority::TemporarilyUnavailable) => {
                return Ok(MachineRequestResult::Unavailable);
            }
            Err(error) => return Err(worker::Error::RustError(error.to_string())),
        };
        let caller = match WorkspaceCaller::from_machine_link(connection, request.caller().clone())
        {
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

    #[cfg_attr(
        not(feature = "test-support"),
        expect(
            clippy::unused_async,
            reason = "only the test-support witness awaits before the reply is sent"
        )
    )]
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
