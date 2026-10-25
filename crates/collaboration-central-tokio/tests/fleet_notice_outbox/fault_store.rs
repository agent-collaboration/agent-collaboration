//! Inject a real uniqueness error after the Fleet callback, inside its transaction.
use super::{Io, Native};
use collaboration_central_core::{
    CoreHost, ObjectHost, RegistrationEdits, RegistrationSnapshot, SocketFailure,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{CloseReason, ConnectionId};
use fleet_sync_session::Bytes;
use sqlx_core::{query::query, row::Row};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
pub struct FaultStore {
    pub object: Arc<Native>,
    pub armed: Arc<AtomicBool>,
    pub injected: Arc<AtomicBool>,
}
impl AtomicStore for FaultStore {
    type Scope<'transaction> = <Native as AtomicStore>::Scope<'transaction>;
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
        let armed = self.armed.clone();
        let injected = self.injected.clone();
        self.object.transaction(move |scope|Box::pin(async move {
            let result=run(scope).await?;
            if armed.load(Ordering::SeqCst) {
                let row=query::<sqlx_turso::Turso>("SELECT count(*) AS count FROM workspaces").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
                let count:i64=row.try_get("count").map_err(StoreError::backend)?;
                if count>0 && armed.swap(false,Ordering::SeqCst) {
                    injected.store(true,Ordering::SeqCst);
                    query::<sqlx_turso::Turso>("INSERT INTO feed_readers(reader_id,installation,last_read_position) SELECT reader_id,installation,last_read_position FROM feed_readers LIMIT 1").execute(scope.executor()).await.map_err(StoreError::backend)?;
                    return Err(StoreError::Aborted {reason:"expected a real feed-reader uniqueness error".into()});
                }
            }
            Ok(result)
        })).await
    }
}
pub struct FaultHost {
    pub native: Arc<TokioStorageHost<CoreHost<Io>>>,
    pub store: Arc<FaultStore>,
}
impl ObjectHost for FaultHost {
    type Database = sqlx_turso::Turso;
    type Store = FaultStore;
    type Wake = CoreHost<Io>;
    fn store(&self) -> &Arc<Self::Store> {
        &self.store
    }
    fn wake(&self) -> &Self::Wake {
        self.native.wake()
    }
    async fn with_local_section<Output: Send + 'static>(
        &self,
        run: impl Future<Output = Output> + Send + 'static,
    ) -> Result<Output, collaboration_central_core::LocalSectionFailure> {
        self.native.with_local_section(run).await
    }
    async fn quarantine_connections(&self, connections: Vec<ConnectionId>) {
        self.native.quarantine_connections(connections).await;
    }
    async fn send_frame(&self, to: ConnectionId, frame: Bytes) -> Result<(), SocketFailure> {
        self.native.send_frame(to, frame).await
    }
    async fn close_connection(
        &self,
        to: ConnectionId,
        reason: CloseReason,
    ) -> Result<(), SocketFailure> {
        self.native.close_connection(to, reason).await
    }
    async fn registrations(&self) -> Result<RegistrationSnapshot, SocketFailure> {
        self.native.registrations().await
    }
    async fn admission_origin(
        &self,
        to: ConnectionId,
    ) -> Result<collaboration_central_core::AdmissionOrigin, SocketFailure> {
        self.native.admission_origin(to).await
    }
    async fn admission_binding(
        &self,
        to: ConnectionId,
    ) -> Result<fleet_sync_session::ConnectionBinding, SocketFailure> {
        self.native.admission_binding(to).await
    }
    async fn publish_registrations(
        &self,
        edits: RegistrationEdits,
    ) -> Vec<(ConnectionId, SocketFailure)> {
        self.native.publish_registrations(edits).await
    }
}
