//! Shared session publication runs over central's store/effect adapter.
use crate::FleetKind;

use crate::{
    CoreHost, ObjectPlatform, ObjectRuntime, object_runtime::ObjectWork,
    session_store::CoreSessionStore,
};
use durable_steps::{AtomicStore, ReborrowScope, StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

type SessionRuntimeHandles<Host> = (
    Arc<CoreSessionStore<Host>>,
    Arc<CoreHost<<Host as ObjectPlatform>::Io>>,
);

impl<Host: ObjectPlatform> ObjectRuntime<FleetKind, Host>
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
    pub(crate) fn session_store(&self) -> Result<SessionRuntimeHandles<Host>, StoreError> {
        match &self.work {
            ObjectWork::Fleet {
                host,
                shared_host,
                replies,
                ..
            } => Ok((
                Arc::new(CoreSessionStore::new(
                    host.clone(),
                    shared_host.clone(),
                    replies.clone(),
                )),
                shared_host.clone(),
            )),
            ObjectWork::Copy { .. } => Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            }),
        }
    }

    async fn session_store_for(
        &self,
        connection: fleet_protocol::ConnectionId,
    ) -> Result<SessionRuntimeHandles<Host>, StoreError> {
        let ObjectWork::Fleet {
            host,
            shared_host,
            replies,
            ..
        } = &self.work
        else {
            return Err(StoreError::backend("session requires Fleet"));
        };
        let origin = host
            .admission_origin(connection)
            .await
            .map_err(StoreError::backend)?;
        let shared = Arc::new(shared_host.session_for(origin.installation));
        Ok((
            Arc::new(CoreSessionStore::new(
                host.clone(),
                shared.clone(),
                replies.clone(),
            )),
            shared,
        ))
    }
    /// Receive a frame through the shared session rules and committed effects.
    pub async fn on_socket_opened(
        &self,
        binding: fleet_session::ConnectionBinding,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        let (store, shared_host) = self.session_store_for(binding.connection).await?;
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    let now = shared_host.now();
                    crate::machine_notice_store::remember_peer(scope, binding.peer, now).await?;
                    let effects = fleet_session::connection_opened(scope, &binding, now)
                        .await
                        .map_err(StoreError::backend)?;
                    scope.stage_transport(effects, Some(binding.connection))
                })
            })
            .await
    }

    /// Receive a frame through the shared session rules and committed effects.
    pub async fn on_frame(
        &self,
        binding: fleet_session::ConnectionBinding,
        frame: Vec<u8>,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        let ObjectWork::Fleet {
            host,
            honored_capabilities,
            ..
        } = &self.work
        else {
            return Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            });
        };
        let honored_capabilities = *honored_capabilities;
        let installation = host
            .admission_origin(binding.connection)
            .await
            .map_err(StoreError::backend)?
            .installation;
        let (store, shared_host) = self.session_store_for(binding.connection).await?;
        let forward_frame = fleet_protocol::FrameId::new(runtime_primitives::IdSource::new_id(
            shared_host.as_ref(),
        ))
        .map_err(StoreError::backend)?;
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    let now = shared_host.now();
                    crate::machine_notice_store::remember_peer(scope, binding.peer, now).await?;
                    let effects = fleet_session::frame_received(
                        scope,
                        &binding,
                        &frame,
                        now,
                        honored_capabilities,
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    scope
                        .stage_frame_effects(
                            effects,
                            binding.connection,
                            installation,
                            now,
                            forward_frame,
                        )
                        .await
                })
            })
            .await
    }

    /// Read the library's actual transport stream without admitting or driving work.
    #[cfg(feature = "conformance")]
    pub async fn conformance_transport_notices(
        &self,
        session: fleet_session::SessionKey,
        after: fleet_protocol::NoticePosition,
    ) -> Result<
        Vec<fleet_session::StoredNotice<board_sync_protocol::BoardConsumerProtocol>>,
        StoreError,
    > {
        let (store, _) = self.session_store()?;
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    fleet_session::notices_after::<_, board_sync_protocol::BoardConsumerProtocol>(
                        scope, &session, after,
                    )
                    .await
                    .map_err(StoreError::backend)
                })
            })
            .await
    }

    #[cfg(feature = "conformance")]
    pub async fn conformance_frame(
        &self,
        binding: fleet_session::ConnectionBinding,
        frame: Vec<u8>,
    ) -> Result<(), StoreError> {
        self.on_frame(binding, frame).await
    }

    pub async fn on_socket_closed(
        &self,
        binding: fleet_session::ConnectionBinding,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        // The callback already carries its admitted binding. Cleanup is local
        // session work and must not rediscover a socket that has disappeared.
        let (store, shared_host) = self.session_store()?;
        let now = shared_host.now();
        let result = store
            .transaction(move |scope| {
                Box::pin(async move {
                    let effects = fleet_session::connection_closed(
                        scope,
                        &binding,
                        fleet_session::CloseCause::Dropped,
                        now,
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    scope.stage_transport(effects, Some(binding.connection))
                })
            })
            .await;
        if let ObjectWork::Fleet { replies, .. } = &self.work {
            replies.fail_connection(binding.connection);
        }
        result
    }

    #[cfg(feature = "conformance")]
    pub async fn conformance_closed(
        &self,
        binding: fleet_session::ConnectionBinding,
    ) -> Result<(), StoreError> {
        self.on_socket_closed(binding).await
    }

    #[cfg(feature = "conformance")]
    pub async fn conformance_initialize_sessions(&self) -> Result<(), StoreError> {
        self.initialize().await.map(|_applied| ())
    }

    /// Read-only test observer of the actual shortened shared scope. The
    /// observer receives no mutable scope and cannot define central step code.
    #[cfg(feature = "conformance")]
    pub async fn conformance_scope_alarm_owner(
        &self,
        at: runtime_primitives::SyncTime,
        inspect: for<'transaction> fn(&<Host::Store as AtomicStore>::Scope<'transaction>) -> usize,
    ) -> Result<usize, StoreError> {
        use durable_steps::JournalScope;
        let (store, _) = self.session_store()?;
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    let identity = inspect(&scope.inner);
                    scope.ensure_wake(at).await?;
                    Ok(identity)
                })
            })
            .await
    }
}
