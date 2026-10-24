//! A verified session's peer is the writer authority; frames cannot choose it.
use super::EffectDriver;
use crate::{
    ObjectHost, PlatformIo,
    copy_write_calls::{CopySessionWrite, WriterCopyRequest},
};
use board_sync_protocol::BoardConsumerProtocol;
use durable_steps::Host as _;
use fleet_sync_protocol::{ConnectionId, DatabaseId, FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(super) async fn answer_copy_write(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        database: DatabaseId,
        request: WriterCopyRequest,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let answer = match self.shared_host.call_identity("copy_write") {
            Ok(identity) => self
                .shared_host
                .dispatch(
                    CopySessionWrite {
                        copy: self.shared_host.authoritative_receiver(),
                        database,
                    },
                    request,
                    identity,
                )
                .await
                .unwrap_or_else(crate::copy_call_failure::answer),
            Err(failure) => crate::copy_call_failure::answer(failure),
        };
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let frame = ServerFrame::<BoardConsumerProtocol>::Answer {
            frame,
            body: OperationAnswer::Replication(answer),
        };
        match serde_json::to_vec(&frame) {
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
