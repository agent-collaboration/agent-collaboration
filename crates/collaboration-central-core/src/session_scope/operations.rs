//! An admitted frame's operation answer leaves through the session transport.
use super::{
    AtomicStore, ConnectionId, CoreSessionScope, ObjectHost, PlatformIo, ReborrowScope,
    SessionTransportEffect, StoreError,
};
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{FrameId, OperationAnswer, ServerFrame};

impl<Host: ObjectHost, Io: PlatformIo> CoreSessionScope<'_, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        durable_steps::storage::TransactionScope<Database = Host::Database>,
{
    pub(super) fn stage_operation_answer(
        &mut self,
        connection: ConnectionId,
        frame: FrameId,
        body: OperationAnswer<BoardConsumerProtocol>,
    ) -> Result<(), StoreError> {
        self.transport.push(SessionTransportEffect::Frame {
            connection,
            bytes: serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
                frame,
                body,
            })
            .map_err(StoreError::backend)?
            .into(),
        });
        Ok(())
    }
}
