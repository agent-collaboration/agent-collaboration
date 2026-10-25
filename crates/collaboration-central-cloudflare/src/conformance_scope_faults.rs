//! One test-build wrapper around the real shared store, never a replacement.

use collaboration_central_conformance::RuntimeFaults;
use durable_steps::OperationFailure;
use durable_steps::{
    AtomicStore, BoxFuture, Host, ReborrowScope, StoreError,
    durable_object_store::DurableObjectStore, storage::TransactionScope,
};
use fleet_sync_protocol::CopyUnavailableReason;
use sqlx_durable_object::{Do, DoExecutor};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct ScopeFaultStore<SharedHost: Host> {
    inner: DurableObjectStore<SharedHost>,
    lose_rollback_reply: AtomicBool,
    reply_loss_observed: AtomicBool,
    runtime_faults: Arc<RuntimeFaults>,
}
impl<SharedHost: Host> ScopeFaultStore<SharedHost> {
    pub(crate) fn new(inner: DurableObjectStore<SharedHost>) -> Self {
        Self {
            inner,
            lose_rollback_reply: AtomicBool::new(false),
            reply_loss_observed: AtomicBool::new(false),
            runtime_faults: Arc::default(),
        }
    }
    pub(crate) fn with_faults(
        inner: DurableObjectStore<SharedHost>,
        runtime_faults: Arc<RuntimeFaults>,
    ) -> Self {
        let mut store = Self::new(inner);
        store.runtime_faults = runtime_faults;
        store
    }
    pub async fn migrate(&self) -> Result<(), StoreError> {
        self.inner.migrate().await
    }
    #[cfg(feature = "conformance-faults")]
    pub(crate) fn lose_next_rollback_reply(&self) {
        self.reply_loss_observed.store(false, Ordering::SeqCst);
        self.lose_rollback_reply.store(true, Ordering::SeqCst);
    }
    #[cfg(feature = "conformance-faults")]
    pub(crate) fn reply_loss_observed(&self) -> bool {
        self.reply_loss_observed.load(Ordering::SeqCst)
    }
    fn observed_reply<T>(&self, result: Result<T, StoreError>) -> Result<T, StoreError> {
        if matches!(&result, Err(StoreError::StepRollback { .. }))
            && self.lose_rollback_reply.swap(false, Ordering::SeqCst)
        {
            self.reply_loss_observed.store(true, Ordering::SeqCst);
            return Err(StoreError::backend(
                "test wrapper lost the completed rollback reply",
            ));
        }
        result
    }
}
impl<SharedHost: Host> ReborrowScope for ScopeFaultStore<SharedHost> {
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        DurableObjectStore::<SharedHost>::reborrow(scope)
    }
}
impl<SharedHost: Host> AtomicStore for ScopeFaultStore<SharedHost> {
    type Scope<'transaction> = <DurableObjectStore<SharedHost> as AtomicStore>::Scope<'transaction>;
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
        let inspect_full = self
            .runtime_faults
            .storage_armed()
            .map_err(StoreError::backend)?;
        if !inspect_full {
            return self.observed_reply(self.inner.transaction(run).await);
        }
        let runtime_faults = self.runtime_faults.clone();
        let result = self
            .inner
            .transaction(move |scope| {
                Box::pin(async move {
                    let before = total_changes(scope.executor()).await?;
                    let output = run(scope).await?;
                    let written = total_changes(scope.executor())
                        .await?
                        .checked_sub(before)
                        .ok_or_else(|| StoreError::backend("total_changes moved backwards"))?;
                    if runtime_faults
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
            .await;
        self.observed_reply(result)
    }
}
async fn total_changes<'executor>(executor: impl DoExecutor<'executor>) -> Result<u64, StoreError> {
    use sqlx_core::row::Row;
    let row = sqlx_core::query::query::<Do>("SELECT CAST(total_changes() AS TEXT) AS changes")
        .fetch_one(executor)
        .await
        .map_err(StoreError::backend)?;
    row.try_get::<String, _>("changes")
        .map_err(StoreError::backend)?
        .parse()
        .map_err(StoreError::backend)
}

/// Consume the one armed connection barrier at the actual socket capability.
#[cfg(feature = "conformance-faults")]
pub(crate) async fn session_barrier<H: Host>(
    host: &crate::CloudflareStorageHost<H>,
    faults: &RuntimeFaults,
    connection: fleet_sync_protocol::ConnectionId,
) -> worker::Result<()> {
    use collaboration_central_core::ObjectHost;
    faults
        .consume_session_barrier(connection)
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    host.send_frame(connection, b"session-finalize-barrier".to_vec().into())
        .await
        .map_err(|error| worker::Error::RustError(error.to_string()))
}

#[cfg(feature = "conformance-faults")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionJournalObservation {
    operation: Option<durable_steps::storage::OperationRecord>,
    report: Option<durable_steps::storage::StepRecord>,
    attach: Option<durable_steps::storage::StepRecord>,
}

/// Read the real journal using its public adapter contract, without private SQL.
#[cfg(feature = "conformance-faults")]
pub(crate) async fn session_journal<H: Host>(
    host: &crate::CloudflareStorageHost<H>,
    operation: durable_steps::OperationId,
) -> worker::Result<SessionJournalObservation> {
    use collaboration_central_core::ObjectHost;
    use durable_steps::storage::JournalStorage;
    host.store()
        .transaction(move |scope| {
            Box::pin(async move {
                Ok(SessionJournalObservation {
                    operation: scope.operation(operation).await?,
                    report: scope.step(operation, 0).await?,
                    attach: scope.step(operation, 1).await?,
                })
            })
        })
        .await
        .map_err(|error| worker::Error::RustError(error.to_string()))
}
