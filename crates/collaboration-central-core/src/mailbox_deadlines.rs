//! Offline expiry and publication admission share the physical Mailbox transaction.
use crate::{
    MailboxKind, ObjectHost, ObjectRuntime, PlatformIo,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{
    AtomicStore, BoxFuture, JournalScope, StoreError,
    storage::{JournalStorage, TransactionScope},
};
use fleet_sync_delivery::{
    DeliveryLimits, Mailbox, PublishStatus, admit_publications, delivery_rollback,
};
use runtime_primitives::{Clock, SyncTime, WakeScheduler};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

struct MaintainDeadlines {
    now: SyncTime,
    limits: DeliveryLimits,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for MaintainDeadlines
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<SyncTime>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            // Re-read even after an empty initial lookup: a later submission
            // can establish custody in this same already-running object.
            let next = if let Some(peer) = Mailbox::<_, BoardDeliveryTypes>::stored_recipient(scope)
                .await
                .map_err(delivery_rollback)?
            {
                Mailbox::<_, BoardDeliveryTypes>::new(scope, peer, self.limits)
                    .expire_due(self.now)
                    .await
                    .map_err(delivery_rollback)?
            } else {
                None
            };
            let publications = admit_publications::<
                _,
                BoardDeliveryTypes,
                PublishStatus<Store, Driver, BoardDeliveryTypes>,
            >(scope)
            .await
            .map_err(delivery_rollback)?;
            if !publications.is_empty() {
                scope.ensure_wake(self.now).await?;
            }
            let next = match (next, scope.earliest().await?) {
                (Some(deadline), Some(work)) => Some(deadline.min(work)),
                (deadline, None) => deadline,
                (None, work) => work,
            };
            Ok((next, Vec::new()))
        })
    }
}
impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<MailboxKind, Host, Io>
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
    pub(crate) async fn maintain_deadlines(&self) -> Result<(), StoreError> {
        let ObjectWork::Mailbox {
            host,
            shared_host,
            limits,
            ..
        } = &self.work
        else {
            return Err(StoreError::backend(
                "mailbox deadlines reached a different face",
            ));
        };
        let (next, _) = StepDriver::new(host.clone())
            .run(MaintainDeadlines {
                now: shared_host.now(),
                limits: *limits,
            })
            .await?;
        if let Some(at) = next {
            host.wake().wake_at(at).await.map_err(StoreError::backend)?;
        }
        Ok(())
    }
}
