//! A store that keeps the running event's scope across its transactions.
//!
//! Work an event does names itself to the event's scope (see
//! `event_owed`), which is set around each poll of the event's future. A
//! store whose transaction polls the callback as a separate task loses that
//! scope: workerd's `Storage::transaction` runs the callback as its own promise
//! (`worker` wraps it in `future_to_promise`). This store captures the scope
//! when a transaction starts, inside the event's poll, and sets it again around
//! every poll of the callback, so attribution doesn't depend on how the store
//! polls. A store that runs the callback inline is unaffected.

use durable_steps::{AtomicStore, BoxFuture, ReborrowScope, StoreError};
use std::sync::Arc;

pub struct EventScopeStore<Store: AtomicStore> {
    inner: Arc<Store>,
}

impl<Store: AtomicStore> EventScopeStore<Store> {
    pub fn new(inner: Arc<Store>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Store {
        self.inner.as_ref()
    }
}

impl<Store: AtomicStore> AtomicStore for EventScopeStore<Store> {
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
        let scope = crate::event_owed::current();
        self.inner
            .transaction(move |transaction| {
                Box::pin(crate::event_owed::within(scope, callback(transaction)))
            })
            .await
    }
}

impl<Store: ReborrowScope> ReborrowScope for EventScopeStore<Store> {
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        Store::reborrow(scope)
    }
}

#[cfg(test)]
#[path = "event_scope_store_test.rs"]
mod tests;
