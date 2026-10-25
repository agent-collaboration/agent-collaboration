//! A conformance-only late storage failure around the real AtomicStore boundary.

use collaboration_central_conformance::RuntimeFaults;
use durable_steps::{
    AtomicStore, BoxFuture, OperationFailure, ReborrowScope, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::CopyUnavailableReason;
use sqlx_core::{query::query, row::Row};
use std::sync::Arc;

pub struct ConformanceStore<Store> {
    inner: Arc<Store>,
    faults: Arc<RuntimeFaults>,
    #[cfg(test)]
    begin_failures_remaining: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    begin_failures_observed: std::sync::atomic::AtomicUsize,
}
impl<Store> ConformanceStore<Store> {
    pub(crate) fn new(inner: Arc<Store>, faults: Arc<RuntimeFaults>) -> Self {
        Self {
            inner,
            faults,
            #[cfg(test)]
            begin_failures_remaining: Default::default(),
            #[cfg(test)]
            begin_failures_observed: Default::default(),
        }
    }
    #[cfg(test)]
    pub(crate) fn fail_next_begin(&self) {
        self.fail_next_begins(1);
    }
    #[cfg(test)]
    pub(crate) fn fail_next_begins(&self, count: usize) {
        self.begin_failures_observed
            .store(0, std::sync::atomic::Ordering::SeqCst);
        self.begin_failures_remaining
            .store(count, std::sync::atomic::Ordering::SeqCst);
    }
    #[cfg(test)]
    pub(crate) fn begin_failures(&self) -> usize {
        self.begin_failures_observed
            .load(std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn begin_failed(&self) -> bool {
        self.begin_failures() > 0
    }
}
impl<Store> AtomicStore for ConformanceStore<Store>
where
    Store: AtomicStore,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = sqlx_turso::Turso>,
{
    type Scope<'transaction> = Store::Scope<'transaction>;
    async fn transaction<Output, Callback>(&self, run: Callback) -> Result<Output, StoreError>
    where
        Output: Send + 'static,
        Callback: for<'scope, 'transaction> FnOnce(
                &'scope mut Self::Scope<'transaction>,
            )
                -> BoxFuture<'scope, Result<Output, StoreError>>
            + Send
            + 'static,
    {
        #[cfg(test)]
        if self
            .begin_failures_remaining
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |remaining| remaining.checked_sub(1),
            )
            .is_ok()
        {
            self.begin_failures_observed
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(StoreError::backend("injected native alarm begin failure"));
        }
        let faults = self.faults.clone();
        if !faults.storage_armed().map_err(StoreError::backend)? {
            return self.inner.transaction(run).await;
        }
        self.inner
            .transaction(move |scope| {
                Box::pin(async move {
                    let before = changes(scope).await?;
                    let output = run(scope).await?;
                    let written = changes(scope)
                        .await?
                        .checked_sub(before)
                        .ok_or_else(|| StoreError::backend("total_changes moved backwards"))?;
                    if faults
                        .fail_after_writes(written)
                        .map_err(StoreError::backend)?
                    {
                        let (code, detail) = CopyUnavailableReason::StorageFull.encode_retry();
                        return Err(StoreError::StepRollback {
                            failure: Box::new(OperationFailure::Refused { code, detail }),
                        });
                    }
                    Ok(output)
                })
            })
            .await
    }
}
impl<Store> ReborrowScope for ConformanceStore<Store>
where
    Store: ReborrowScope,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = sqlx_turso::Turso>,
{
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        Store::reborrow(scope)
    }
}
async fn changes(
    scope: &mut impl TransactionScope<Database = sqlx_turso::Turso>,
) -> Result<u64, StoreError> {
    let row = query::<sqlx_turso::Turso>("SELECT CAST(total_changes() AS TEXT) AS changes")
        .fetch_one(scope.executor())
        .await
        .map_err(StoreError::backend)?;
    row.try_get::<String, _>("changes")
        .map_err(StoreError::backend)?
        .parse()
        .map_err(StoreError::backend)
}
