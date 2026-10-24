//! Copy failures stay in their correlated answer and leave the session usable.

use super::EffectDriver;
use crate::{
    ObjectHost, PlatformIo,
    copy_read_calls::{CopyReadRequest, CopySessionRead},
};
use board_sync_protocol::BoardConsumerProtocol;
use durable_steps::Host as _;
use fleet_sync_protocol::{ConnectionId, DatabaseId, FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(super) async fn answer_copy_read(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        database: DatabaseId,
        request: CopyReadRequest,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let answer = match self.shared_host.call_identity("copy_read") {
            Ok(identity) => self
                .shared_host
                .dispatch(
                    CopySessionRead {
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
        // The copy call may have yielded to a replacement or a revocation.
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
