use super::EventScopeStore;
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use crate::notice_appends::{scoped, take};
use board_sync_protocol::BoardNotice;
use durable_steps::{AtomicStore, BoxFuture, StoreError};
use fleet_protocol::PeerId;
use std::{collections::BTreeSet, sync::Arc};

/// Runs each transaction on its own task, as workerd's storage transaction
/// runs its callback as a separate promise.
struct DetachedStore<Store>(Arc<Store>);

impl<Store: AtomicStore + Send + Sync + 'static> AtomicStore for DetachedStore<Store> {
    type Scope<'transaction> = Store::Scope<'transaction>;

    async fn transaction<Output, Callback>(&self, callback: Callback) -> Result<Output, StoreError>
    where
        Output: Send + 'static,
        Callback: for<'scope, 'transaction> FnOnce(
                &'scope mut Self::Scope<'transaction>,
            )
                -> BoxFuture<'scope, Result<Output, StoreError>>
            + Send
            + 'static,
    {
        let inner = Arc::clone(&self.0);
        tokio::spawn(async move { inner.transaction(callback).await })
            .await
            .map_err(StoreError::backend)?
    }
}

/// Append one notice for `peer` through `store`, inside one event.
async fn appended_in_event<Store: AtomicStore>(
    store: &Store,
    peer: PeerId,
) -> Result<BTreeSet<PeerId>, StoreError>
where
    for<'transaction> Store::Scope<'transaction>:
        durable_steps::storage::TransactionScope<Database = sqlx_turso::Turso>,
{
    scoped(async move {
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    crate::machine_notice_store::append(
                        scope,
                        peer,
                        durable_steps::Uuid::now_v7().to_string(),
                        BoardNotice::ProjectListChanged {},
                        NOW,
                    )
                    .await
                    .map(|_| ())
                })
            })
            .await?;
        Ok(take())
    })
    .await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_append_in_a_detached_transaction_still_names_its_machine() -> Result<(), TestError> {
    // Arrange: a store that polls its transaction callback on another task.
    let fixture = FleetStorageFixture::open().await?;
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    fixture
        .enroll(peer, "019f0000-0000-7000-8000-000000008141")
        .await?;
    let detached = Arc::new(DetachedStore(Arc::clone(&fixture.object)));

    // Act
    let lost = appended_in_event(detached.as_ref(), peer).await?;
    let kept = appended_in_event(&EventScopeStore::new(Arc::clone(&detached)), peer).await?;

    // Assert: the bare detached store loses the event's scope (the workerd
    // defect); carried across the transaction, the append names its machine.
    assert!(lost.is_empty(), "the detached task saw the event's scope");
    assert_eq!(kept, BTreeSet::from([peer]));
    fixture.close().await?;
    Ok(())
}
