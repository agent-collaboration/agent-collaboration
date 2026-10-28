//! Fleet request provenance is bound at admission before leaving the session owner.
use super::EffectDriver;
use crate::{
    ObjectHost, PlatformIo,
    fleet_metadata_calls::{FleetMetadata, FleetMetadataRequest},
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardPayload, FleetRequestEnvelope,
};
use durable_steps::Host as _;
use fleet_sync_protocol::{ConnectionId, FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(super) async fn answer_fleet_request(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        envelope: FleetRequestEnvelope,
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
        let request_id = envelope.request_id.clone();
        let answer = if let Some(request) = FleetMetadataRequest::from_envelope(envelope, origin) {
            let Ok(identity) = self.shared_host.call_identity("fleet_request") else {
                self.fail_connection(connection).await;
                return;
            };
            let decision = self
                .shared_host
                .dispatch(FleetMetadata, request, identity)
                .await;
            match decision.and_then(|decision| {
                BoardPayload::encode(&decision).map_err(|_| durable_steps::CallFailure::AnswerLost)
            }) {
                Ok(decision) => OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
                    request_id,
                    decision,
                }),
                Err(_) => {
                    self.fail_connection(connection).await;
                    return;
                }
            }
        } else {
            OperationAnswer::InvalidShape {
                field: "fleetRequest".into(),
            }
        };
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let bytes = serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
            frame,
            body: answer,
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
