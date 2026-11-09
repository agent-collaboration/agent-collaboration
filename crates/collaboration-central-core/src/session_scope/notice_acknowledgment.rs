//! NoticesApplied: a negotiated Router says how far it applied its notices.
//!
//! The session rule already closed any connection that sent this without
//! negotiating it. Here the position is stored (it only moves forward), the
//! answer goes out in the same event (R1), and once the Router has caught up
//! central no longer owes it a resend.
use super::{
    AtomicStore, ColumnIndex, ConnectionId, CoreSessionScope, Database, Decode, Encode,
    IntoArguments, ObjectPlatform, ReborrowScope, StoreError, SyncTime, TransactionScope, Type,
};
use fleet_protocol::{FrameId, NoticePosition, OperationAnswer};
use fleet_session::SessionKey;

impl<Host: ObjectPlatform> CoreSessionScope<'_, Host>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(super) async fn acknowledge_notices(
        &mut self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        through: NoticePosition,
        now: SyncTime,
    ) -> Result<(), StoreError> {
        let progress =
            crate::machine_notice_progress::record_applied(self, session.peer, through, now)
                .await?;
        if progress.is_none_or(|progress| !progress.behind()) {
            crate::work_attempts::succeeded(
                self,
                session.peer.as_uuid(),
                crate::work_attempts::AttemptKind::NoticeDelivery,
            )
            .await?;
        }
        // The answer echoes what is now stored: a lower position than the stored
        // one, or one past the issued head, is answered with the stored value.
        let applied = progress
            .and_then(|progress| progress.applied)
            .map_or(through, NoticePosition::new);
        self.stage_operation_answer(
            connection,
            frame,
            OperationAnswer::NoticesApplied { through: applied },
        )
    }
}
