//! Inject a real uniqueness error after the Fleet callback, inside its transaction.
use collaboration_central_core::{
    CoreHost, ObjectIo, ObjectPlatform, RegistrationEdits, RegistrationSnapshot, SocketFailure,
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_protocol::{CloseReason, ConnectionId};
use fleet_session::Bytes;
use fleet_tokio_objects::TokioObject;
use sqlx_core::{query::query, row::Row};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub struct FaultStore<Io: ObjectIo> {
    pub object: Arc<TokioObject<CoreHost<Io>>>,
    pub armed: Arc<AtomicBool>,
    pub injected: Arc<AtomicBool>,
}
impl<Io: ObjectIo> durable_steps::ReborrowScope for FaultStore<Io> {
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        TokioObject::<CoreHost<Io>>::reborrow(scope)
    }
}
impl<Io: ObjectIo> AtomicStore for FaultStore<Io> {
    type Scope<'transaction> = <TokioObject<CoreHost<Io>> as AtomicStore>::Scope<'transaction>;
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
                let row=query::<sqlx_turso::Turso>("SELECT count(*) AS count FROM fleet_workspaces").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
                let count:i64=row.try_get("count").map_err(StoreError::backend)?;
                if count>0 && armed.swap(false,Ordering::SeqCst) {
                    injected.store(true,Ordering::SeqCst);
                    query::<sqlx_turso::Turso>("INSERT INTO fleet_feed_readers(reader_id,installation,last_read_position,created_at,updated_at) SELECT reader_id,installation,last_read_position,created_at,updated_at FROM fleet_feed_readers LIMIT 1").execute(scope.executor()).await.map_err(StoreError::backend)?;
                    return Err(StoreError::Aborted {reason:"expected a real feed-reader uniqueness error".into()});
                }
            }
            Ok(result)
        })).await
    }
}
pub struct FaultHost<Io: ObjectIo> {
    pub native: Arc<crate::io_alarm::IoPlatform<Io>>,
    pub store: Arc<FaultStore<Io>>,
}
impl<Io: ObjectIo> ObjectPlatform for FaultHost<Io> {
    type Database = sqlx_turso::Turso;
    type Store = FaultStore<Io>;
    type Alarm = crate::io_alarm::IoAlarm<Io>;
    type Io = Io;
    fn store(&self) -> &Arc<Self::Store> {
        &self.store
    }
    fn alarm(&self) -> &Self::Alarm {
        self.native.alarm()
    }
    fn io(&self) -> &std::sync::Arc<Self::Io> {
        self.native.io()
    }
    async fn with_local_section<Output: Send + 'static>(
        &self,
        run: collaboration_central_core::LocalSection<Output>,
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
    ) -> Result<fleet_session::ConnectionBinding, SocketFailure> {
        self.native.admission_binding(to).await
    }
    async fn publish_registrations(
        &self,
        edits: RegistrationEdits,
    ) -> Vec<(ConnectionId, SocketFailure)> {
        self.native.publish_registrations(edits).await
    }
}
