//! Publishing a machine's retained notices to its session after a position.
//!
//! The Fleet's notice push and resend call this inside a session-store
//! transaction, so each notice is staged as session transport and leaves only
//! once that transaction commits.
use super::{
    AtomicStore, ColumnIndex, CoreSessionScope, Database, Decode, Encode, IntoArguments,
    ObjectPlatform, ReborrowScope, StoreError, SyncTime, TransactionScope, Type,
};
use crate::notice_push::PUSH_LIMIT;
use board_sync_protocol::{BoardConsumerProtocol, NoticeRead};
use fleet_protocol::{NoticePosition, PeerId};

impl<Host: ObjectPlatform> CoreSessionScope<'_, Host>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// Publish up to 32 retained notices after `after` to the peer's current
    /// session; answers the last position sent (or `after` when nothing was).
    pub(crate) async fn publish_notices_after(
        &mut self,
        peer: PeerId,
        after: u64,
        now: SyncTime,
    ) -> Result<u64, StoreError> {
        let page = match crate::machine_notice_store::read(
            self,
            peer,
            Some(NoticePosition::new(after)),
            now,
        )
        .await?
        {
            NoticeRead::CursorExpired { floor } => {
                crate::machine_notice_store::read(self, peer, Some(floor), now).await?
            }
            read => read,
        };
        let NoticeRead::Page { notices, .. } = page else {
            return Ok(after);
        };
        let mut through = after;
        for notice in notices.into_iter().take(PUSH_LIMIT) {
            through = notice.position.value();
            let effects = fleet_session::publish_notice_to_peer::<_, BoardConsumerProtocol>(
                self,
                &peer,
                fleet_protocol::Notice::Consumer(notice),
                now,
            )
            .await
            .map_err(StoreError::backend)?;
            self.stage_transport(effects, None)?;
        }
        Ok(through)
    }
}
