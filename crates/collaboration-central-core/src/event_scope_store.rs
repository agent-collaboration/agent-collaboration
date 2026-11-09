//! A store that keeps the running event's scope across its transactions.
//!
//! Work an event does names itself to the event's scope (see
//! `event_owed`), which is set around each poll of the event's future. A
//! store whose transaction polls the callback as a separate task loses that
//! scope: workerd's `Storage::transaction` runs the callback as its own promise
//! (`worker` wraps it in `future_to_promise`). This store captures the scope
//! when a transaction starts, inside the event's poll, and sets it again around
//! every poll of the callback, so attribution doesn't depend on how the store
//! polls. A store that runs the callback inline is unaffected. Conformance
//! builds carry the event's trace the same way, for its send bindings.
//!
//! The same carry wraps every local section ([`carried`]): workerd's
//! `blockConcurrencyWhile` also polls its future as its own promise.

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
        let carried = CarriedScope::capture();
        self.inner
            .transaction(move |transaction| Box::pin(carried.within(callback(transaction))))
            .await
    }
}

/// The running event's scopes, captured inside its poll: what it owes and, in
/// conformance builds, its trace (S1 send bindings).
pub(crate) struct CarriedScope {
    owed: Option<crate::event_owed::Shared>,
    #[cfg(feature = "conformance")]
    trace: Option<Arc<crate::event_trace::EventTrace>>,
}

impl CarriedScope {
    /// Capture the scopes of the event being polled now.
    pub(crate) fn capture() -> Self {
        Self {
            owed: crate::event_owed::current(),
            #[cfg(feature = "conformance")]
            trace: crate::event_trace::current_trace(),
        }
    }

    /// Run `inner` inside the captured scopes on every poll, wherever it is polled.
    pub(crate) fn within<F: std::future::Future + Send>(
        self,
        inner: F,
    ) -> impl std::future::Future<Output = F::Output> + Send {
        let carried = crate::event_owed::within(self.owed, inner);
        #[cfg(feature = "conformance")]
        let carried = crate::event_trace::within_trace(self.trace, carried);
        carried
    }
}

/// `inner` carrying the scopes of the event that creates it, for a platform
/// local section that may poll it as its own task.
pub(crate) fn carried<F: std::future::Future + Send>(
    inner: F,
) -> impl std::future::Future<Output = F::Output> + Send {
    CarriedScope::capture().within(inner)
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
