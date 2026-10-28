//! Resolve a library holder from committed rows in the registry's own store.

use crate::{
    ObjectHost, generated_enrollment,
    machine_enrollment::service_for_stored_peer,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, NotSentReason, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::PeerId;
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{
    convert::Infallible,
    sync::{Arc, Weak},
};

pub(crate) type PeerDirectory =
    Arc<dyn Fn(PeerId) -> BoxFuture<'static, Result<ServiceId, CallFailure>> + Send + Sync>;

pub(crate) fn bind<Host: ObjectHost>(host: Weak<Host>) -> PeerDirectory
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    Arc::new(move |peer| {
        let host = host.clone();
        Box::pin(async move {
            let host = host.upgrade().ok_or_else(unreachable_peer)?;
            let (service, _) = StepDriver::new(host)
                .run(LookupPeer { peer })
                .await
                .map_err(|_| CallFailure::NotSent {
                    reason: NotSentReason::Overloaded,
                })?;
            service.ok_or_else(unreachable_peer)
        })
    })
}

struct LookupPeer {
    peer: PeerId,
}
impl<Store, Driver> StepCallback<Store, Driver> for LookupPeer
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<ServiceId>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let service = service_for_peer(scope, self.peer).await?;
            Ok((service, Vec::new()))
        })
    }
}

pub(crate) async fn service_for_peer<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
) -> Result<Option<ServiceId>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut rows =
        generated_enrollment::read_enrollments(scope, None, None, None, Some(peer.to_string()))
            .await
            .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(StoreError::Corrupt {
            field: "machine_enrollments.peer_id".into(),
            message: "duplicate peer mapping".into(),
        });
    }
    rows.pop()
        .map(|row| service_for_stored_peer(row, peer))
        .transpose()
}
fn unreachable_peer() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
