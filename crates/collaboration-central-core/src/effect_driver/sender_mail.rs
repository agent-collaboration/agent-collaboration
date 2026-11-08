//! Sender reconciliation enters the owning Fleet after the session transaction releases.
use super::EffectDriver;
use crate::{
    ObjectPlatform,
    mail_sender_calls::{SenderFleetMail, SenderMailRequest},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::Host as _;
use fleet_protocol::{ConnectionId, DeliveryRequest, FrameId, OperationAnswer, ServerFrame};
use fleet_session::SessionKey;

impl<Host: ObjectPlatform> EffectDriver<Host> {
    pub(super) async fn answer_sender_mail(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        request: Box<DeliveryRequest<BoardDeliveryTypes>>,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        if self.host.admission_origin(connection).await.is_err() {
            self.fail_connection(connection).await;
            return;
        }
        let identity = request.call().clone();
        let answer = self
            .shared_host
            .dispatch(
                SenderFleetMail,
                SenderMailRequest { session, request },
                identity,
            )
            .await;
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
