//! Observe actual rejected drive tokens after the real store boundary returns.

use durable_steps::{AtomicStore, BoxFuture, OperationId, ReborrowScope, StoreError};
use std::sync::{Arc, Mutex};

pub struct DriveTokenObserver<Store: AtomicStore> {
    inner: Arc<Store>,
    rejected_operations: Mutex<Vec<OperationId>>,
}
impl<Store: AtomicStore> DriveTokenObserver<Store> {
    pub fn new(inner: Arc<Store>) -> Self {
        Self {
            inner,
            rejected_operations: Mutex::default(),
        }
    }
    pub fn inner(&self) -> &Store {
        self.inner.as_ref()
    }
    pub fn rejected_operations(&self) -> Result<Vec<OperationId>, StoreError> {
        Ok(self
            .rejected_operations
            .lock()
            .map_err(StoreError::backend)?
            .clone())
    }
}
impl<Store: AtomicStore> AtomicStore for DriveTokenObserver<Store> {
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
        let result = self.inner.transaction(callback).await;
        if let Err(StoreError::StaleDriveToken { id }) = &result
            && let Ok(mut rejected) = self.rejected_operations.lock()
        {
            rejected.push(*id);
        }
        // Never replace an outcome or claim a rollback from callback state.
        result
    }
}
impl<Store: ReborrowScope> ReborrowScope for DriveTokenObserver<Store> {
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        Store::reborrow(scope)
    }
}
