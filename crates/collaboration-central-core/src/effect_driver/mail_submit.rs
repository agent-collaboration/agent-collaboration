//! Mail's origin comes from the admitted socket, outside its local transaction.

use super::EffectDriver;
use crate::{
    ObjectPlatform,
    mail_submit_calls::{FleetMailSubmit, MailSubmitReply, MailSubmitRequest},
};
use board_sync_protocol::{BoardConsumerAnswer, BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::Host as _;
use fleet_protocol::{
    ConnectionId, DeliveryEnvelope, Epoch, FrameId, OperationAnswer, ServerFrame,
};
use fleet_session::SessionKey;

impl<Host: ObjectPlatform> EffectDriver<Host> {
    pub(super) async fn answer_mail_submit(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        item: Box<DeliveryEnvelope<BoardDeliveryTypes>>,
        origin_epoch: Option<Epoch>,
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
        let Ok(identity) = self.shared_host.call_identity("mail_submit") else {
            self.fail_connection(connection).await;
            return;
        };
        let answer = self
            .shared_host
            .dispatch(
                FleetMailSubmit,
                MailSubmitRequest {
                    session,
                    origin,
                    item,
                    origin_epoch,
                },
                identity,
            )
            .await;
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let answer = match answer {
            Ok(MailSubmitReply::Answer(answer)) => {
                OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(answer))
            }
            Ok(MailSubmitReply::InvalidShape { field }) => OperationAnswer::InvalidShape { field },
            Err(_) => {
                self.fail_connection(connection).await;
                return;
            }
        };
        match serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
            frame,
            body: answer,
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
