//! Translate Fleet-owned read scopes; unrelated cursors remain invalid.
use super::{
    ConnectionId, CoreSessionScope, ObjectHost, PlatformIo, ReborrowScope, SessionTransportEffect,
    StoreError,
};
use crate::fleet_read_calls::FleetReadRequest;
use board_sync_protocol::{BoardConsumerProtocol, BoardReadCursor, BoardReadScope};
use fleet_sync_protocol::{FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::{SessionEffect, SessionKey};
impl<Host: ObjectHost, Io: PlatformIo> CoreSessionScope<'_, Host, Io>
where
    Host::Store: ReborrowScope,
{
    pub(super) fn stage_fleet_read(
        &mut self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        scope: BoardReadScope,
        cursor: Option<BoardReadCursor>,
    ) -> Result<(), StoreError> {
        let request = match (scope, cursor) {
            (BoardReadScope::Directory { query }, None) => {
                match query.decode::<fleet_admission::DirectoryQuery>() {
                    Ok(query) => FleetReadRequest::Directory { query },
                    Err(_) => return self.invalid_fleet_read(connection, frame, "directory.query"),
                }
            }
            (BoardReadScope::FleetRecords, None) => FleetReadRequest::Feed {
                after: message_board::FleetPosition::ZERO,
            },
            (BoardReadScope::FleetRecords, Some(BoardReadCursor::Fleet(after))) => {
                FleetReadRequest::Feed { after }
            }
            (BoardReadScope::FleetSnapshot, None) => FleetReadRequest::Snapshot { cursor: None },
            (BoardReadScope::FleetSnapshot, Some(BoardReadCursor::FleetSnapshot(cursor))) => {
                FleetReadRequest::Snapshot {
                    cursor: Some(cursor),
                }
            }
            _ => {
                return self.invalid_fleet_read(connection, frame, "cursor");
            }
        };
        self.transport.push(SessionTransportEffect::FleetRead {
            connection,
            session,
            frame,
            request,
        });
        Ok(())
    }

    fn invalid_fleet_read(
        &mut self,
        connection: ConnectionId,
        frame: FrameId,
        field: &str,
    ) -> Result<(), StoreError> {
        self.stage_transport(
            vec![SessionEffect::Send {
                connection,
                frame: ServerFrame::<BoardConsumerProtocol>::Answer {
                    frame,
                    body: OperationAnswer::InvalidShape {
                        field: field.into(),
                    },
                },
            }],
            None,
        )
    }
}
