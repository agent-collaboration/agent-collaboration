//! The host adapter borrows the file owner's transaction into the library scope.
use crate::InteractionStore;
use durable_steps::{
    AtomicStore, BoxFuture, Host, ReborrowScope, StoreError,
    turso_store::{TursoScope, TursoStore},
};
use sqlx::Connection;
use std::sync::Arc;

pub struct InteractionAtomicStore<THost: Host> {
    owner: Arc<InteractionStore>,
    host: Arc<THost>,
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    committed_writes: tokio::sync::watch::Sender<u64>,
}

impl<THost: Host> InteractionAtomicStore<THost> {
    pub fn new(owner: Arc<InteractionStore>, host: Arc<THost>) -> Self {
        Self {
            owner,
            host,
            #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
            committed_writes: tokio::sync::watch::channel(0).0,
        }
    }

    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    pub fn runtime_test_observe_committed_writes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.committed_writes.subscribe()
    }
}

impl<THost: Host> AtomicStore for InteractionAtomicStore<THost> {
    type Scope<'transaction> = TursoScope<'transaction, THost>;

    async fn transaction<TOutput, TRun>(&self, run: TRun) -> Result<TOutput, StoreError>
    where
        TOutput: Send + 'static,
        TRun: for<'scope, 'transaction> FnOnce(
                &'scope mut Self::Scope<'transaction>,
            )
                -> BoxFuture<'scope, Result<TOutput, StoreError>>
            + Send
            + 'static,
    {
        let mut connection = self.owner.writer.lock().await;
        #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
        let previous_changes: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(&mut *connection)
            .await
            .map_err(StoreError::backend)?;
        let mut transaction = connection
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(StoreError::backend)?;
        let mut scope = TursoScope::new(&mut transaction, &*self.host);
        match run(&mut scope).await {
            Ok(output) => {
                transaction.commit().await.map_err(StoreError::backend)?;
                #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
                {
                    let committed_changes: i64 = sqlx::query_scalar("SELECT total_changes()")
                        .fetch_one(&mut *connection)
                        .await
                        .map_err(StoreError::backend)?;
                    if committed_changes != previous_changes {
                        self.committed_writes
                            .send_modify(|counter| *counter = counter.wrapping_add(1));
                    }
                }
                Ok(output)
            }
            Err(error) => {
                transaction.rollback().await.map_err(StoreError::backend)?;
                Err(error)
            }
        }
    }
}

impl<THost: Host> ReborrowScope for InteractionAtomicStore<THost> {
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        // This delegates a borrowed scope only; no library TursoStore is opened.
        <TursoStore<THost> as ReborrowScope>::reborrow(scope)
    }
}
