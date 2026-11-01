//! Automation calls leave the Session owner with its real admitted installation.
use super::EffectDriver;
use crate::{
    ObjectHost, PlatformIo,
    fleet_automation_calls::{FleetAutomation, FleetAutomationRequest},
};
use board_sync_protocol::{BoardConsumerAnswer, BoardConsumerProtocol, BoardPayload};
use durable_steps::Host as _;
use fleet_sync_protocol::{ConnectionId, FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(super) async fn answer_automation_request(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        call: BoardPayload,
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
        let Ok(identity) = self.shared_host.call_identity("automation_request") else {
            self.fail_connection(connection).await;
            return;
        };
        let result = self
            .shared_host
            .dispatch(
                FleetAutomation,
                FleetAutomationRequest {
                    installation: origin.installation,
                    call,
                },
                identity,
            )
            .await;
        let answer = match result.and_then(|answer| {
            BoardPayload::encode(&answer).map_err(|_| durable_steps::CallFailure::AnswerLost)
        }) {
            Ok(answer) => BoardConsumerAnswer::Automation(answer),
            Err(_) => {
                self.fail_connection(connection).await;
                return;
            }
        };
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let bytes = serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
            frame,
            body: OperationAnswer::Consumer(answer),
        });
        match bytes {
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
