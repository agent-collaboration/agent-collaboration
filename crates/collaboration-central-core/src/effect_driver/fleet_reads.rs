//! Fleet reads leave the connection's local section before crossing the DAG.

use super::EffectDriver;
use crate::{
    ObjectHost, PlatformIo,
    fleet_read_calls::{FleetRead, FleetReadRequest},
    fleet_runtime::FleetRequestEffects,
};
use board_sync_protocol::{BoardConsumerAnswer, BoardConsumerProtocol, BoardReadPage};
use durable_steps::{CallFailure, Host as _};
use fleet_sync_protocol::{ConnectionId, FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;
use message_board::RequestEffectsRequest;

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(super) async fn answer_fleet_read(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        request: FleetReadRequest,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let answer = async {
            self.shared_host
                .dispatch(
                    FleetRead,
                    request,
                    self.shared_host.call_identity("fleet_read")?,
                )
                .await
        }
        .await;
        self.send_fleet_read_answer(connection, session, frame, answer)
            .await;
    }
    pub(super) async fn answer_fleet_effects(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        request: RequestEffectsRequest,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let Ok(identity) = self.shared_host.call_identity("fleet_effects") else {
            self.fail_connection(connection).await;
            return;
        };
        let answer = self
            .shared_host
            .dispatch(FleetRequestEffects, request, identity)
            .await;
        self.send_fleet_read_answer(connection, session, frame, answer)
            .await;
    }
    async fn send_fleet_read_answer(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        answer: Result<BoardReadPage, CallFailure>,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let Ok(page) = answer else {
            self.fail_connection(connection).await;
            return;
        };
        let reply = ServerFrame::<BoardConsumerProtocol>::Answer {
            frame,
            body: OperationAnswer::Consumer(BoardConsumerAnswer::Read(page)),
        };
        match serde_json::to_vec(&reply) {
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
