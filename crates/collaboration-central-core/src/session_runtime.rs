//! Shared session publication runs over central's store/effect adapter.
use crate::FleetKind;

use crate::{
    CoreHost, ObjectHost, ObjectRuntime, PlatformIo, object_runtime::ObjectWork,
    session_store::CoreSessionStore,
};
use durable_steps::{AtomicStore, ReborrowScope, StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

type SessionRuntimeHandles<Host, Io> = (Arc<CoreSessionStore<Host, Io>>, Arc<CoreHost<Io>>);

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) fn session_store(&self) -> Result<SessionRuntimeHandles<Host, Io>, StoreError> {
        let ObjectWork::Fleet {
            host, shared_host, ..
        } = &self.work;
        Ok((
            Arc::new(CoreSessionStore::new(host.clone())),
            shared_host.clone(),
        ))
    }

    async fn session_store_for(
        &self,
        connection: fleet_sync_protocol::ConnectionId,
    ) -> Result<SessionRuntimeHandles<Host, Io>, StoreError> {
        let ObjectWork::Fleet {
            host, shared_host, ..
        } = &self.work;
        let origin = host
            .admission_origin(connection)
            .await
            .map_err(StoreError::backend)?;
        let shared = Arc::new(shared_host.session_for(origin.installation));
        Ok((Arc::new(CoreSessionStore::new(host.clone())), shared))
    }
    /// Receive a frame through the shared session rules and committed effects.
    pub async fn on_socket_opened(
        &self,
        binding: fleet_sync_session::ConnectionBinding,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        let (store, shared_host) = self.session_store_for(binding.connection).await?;
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    let now = shared_host.now();
                    let effects = fleet_sync_session::connection_opened(scope, &binding, now)
                        .await
                        .map_err(StoreError::backend)?;
                    scope.stage_transport(effects)
                })
            })
            .await
    }

    /// Receive a frame through the shared session rules and committed effects.
    pub async fn on_frame(
        &self,
        binding: fleet_sync_session::ConnectionBinding,
        frame: Vec<u8>,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        let (store, shared_host) = self.session_store_for(binding.connection).await?;
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    let now = shared_host.now();
                    let effects = fleet_sync_session::frame_received(scope, &binding, &frame, now)
                        .await
                        .map_err(StoreError::backend)?;
                    scope.stage_frame_effects(effects, binding.connection)
                })
            })
            .await
    }

    pub async fn on_socket_closed(
        &self,
        binding: fleet_sync_session::ConnectionBinding,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        // The callback already carries its admitted binding. Cleanup is local
        // session work and must not rediscover a socket that has disappeared.
        let (store, shared_host) = self.session_store()?;
        let now = shared_host.now();
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    let effects = fleet_sync_session::connection_closed(
                        scope,
                        &binding,
                        fleet_sync_session::CloseCause::Dropped,
                        now,
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    scope.stage_transport(effects)
                })
            })
            .await
    }
}
