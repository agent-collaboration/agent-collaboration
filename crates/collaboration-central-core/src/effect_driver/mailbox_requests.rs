//! Recipient custody is called after the session's local transaction releases.
use super::EffectDriver;
use crate::{
    ObjectHost, PlatformIo,
    mailbox_requests::{MailboxRelayRequest, MailboxRequest},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::Host as _;
use fleet_sync_protocol::{ConnectionId, DeliveryRequest, FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(super) async fn answer_mailbox_request(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        request: Box<DeliveryRequest<BoardDeliveryTypes>>,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let origin = match self.host.admission_origin(connection).await {
            Ok(origin) => origin,
            Err(_) => {
                self.fail_connection(connection).await;
                return;
            }
        };
        let identity = request.call().clone();
        let sender_operation = matches!(
            request.as_ref(),
            DeliveryRequest::Query { .. }
                | DeliveryRequest::Settle { .. }
                | DeliveryRequest::Retire { .. }
        );
        let answer = if sender_operation {
            self.shared_host
                .dispatch(
                    crate::mail_sender_calls::SenderMailboxRegistry,
                    crate::mail_sender_calls::SenderMailboxRequest { session, request },
                    identity,
                )
                .await
        } else {
            self.shared_host
                .dispatch(
                    MailboxRequest {
                        installation: origin.installation,
                        recipient: session.peer,
                    },
                    MailboxRelayRequest {
                        actor: session.peer,
                        request,
                    },
                    identity,
                )
                .await
        };
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let Ok(answer) = answer else {
            self.fail_connection(connection).await;
            return;
        };
        match serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
            frame,
            body: OperationAnswer::Delivery(answer),
        }) {
            Ok(bytes) => {
                if self
                    .host
                    .send_frame(connection, bytes.into())
                    .await
                    .is_err()
                {
                    self.fail_connection(connection).await;
                }
            }
            Err(_) => self.fail_connection(connection).await,
        }
    }
}
