//! Shared session publication runs over central's store/effect adapter.

use crate::{
    CoreHost, ObjectHost, ObjectRuntime, PlatformIo, SessionKind, object_runtime::ObjectWork,
    session_store::CoreSessionStore,
};
use durable_steps::{
    AtomicStore, DriveReport, Engine, ReborrowScope, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::ReceiverId;
use fleet_sync_session::PublishSchemaSupport;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

type SessionRuntimeHandles<Host, Io> = (Arc<CoreSessionStore<Host, Io>>, Arc<CoreHost<Io>>);

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<SessionKind, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn session(
        host: Arc<Host>,
        io: Io,
        receiver: ReceiverId,
        installation: message_board::ServiceId,
        holder_answer_timeout: std::time::Duration,
    ) -> Result<Self, StoreError> {
        let io = Arc::new(io);
        let replies = Arc::new(crate::holder_reply_hub::HolderReplyHub::default());
        let publication = crate::mail_status_dispatch::bind(
            Arc::downgrade(&host),
            io.clone(),
            replies.clone(),
            installation.clone(),
            holder_answer_timeout,
        );
        let shared_host = Arc::new(CoreHost::session(
            io,
            receiver,
            installation.clone(),
            publication,
        ));
        Ok(Self {
            work: ObjectWork::Session {
                host,
                shared_host,
                installation,
                holder_answer_timeout,
                replies,
            },
            face: PhantomData,
        })
    }

    pub async fn on_alarm(&self) -> Result<DriveReport, StoreError> {
        use runtime_primitives::{Clock, WakeScheduler};
        if let ObjectWork::Session {
            replies,
            host,
            shared_host,
            ..
        } = &self.work
        {
            replies.expire(shared_host.now());
            if let Some(at) = replies.next_deadline() {
                host.wake().wake_at(at).await.map_err(StoreError::backend)?;
            }
        }
        let (store, shared_host) = self.session_store()?;
        let now = runtime_primitives::Clock::now(shared_host.as_ref());
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    scope.expire_forwards(now).await?;
                    crate::machine_notice_store::prune(scope, now).await?;
                    if let Some(at) = crate::machine_notice_store::next_expiry(scope).await? {
                        durable_steps::JournalScope::ensure_wake(scope, at).await?;
                    }
                    Ok(())
                })
            })
            .await?;
        let mut engine = Engine::new(store, shared_host);
        engine
            .register(crate::machine_notice_delivery::MachineNoticeTask::<Host, Io>(PhantomData))?;
        engine.register(PublishSchemaSupport::<
            CoreSessionStore<Host, Io>,
            Host::Database,
        >::default())?;
        engine.register(crate::session_revocation::EndSessionsTask::<Host, Io>(
            PhantomData,
        ))?;
        engine.register(fleet_sync_delivery::PublishStatus::<
            CoreSessionStore<Host, Io>,
            Host::Database,
            board_sync_protocol::BoardDeliveryTypes,
        >::new())?;
        engine.drive_due().await
    }

    fn session_store(&self) -> Result<SessionRuntimeHandles<Host, Io>, StoreError> {
        match &self.work {
            ObjectWork::Session {
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
            ObjectWork::Fleet { .. }
            | ObjectWork::Registry { .. }
            | ObjectWork::Copy { .. }
            | ObjectWork::Mailbox { .. } => Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            }),
        }
    }

    /// Receive a frame through the shared session rules and committed effects.
    pub async fn on_socket_opened(
        &self,
        binding: fleet_sync_session::ConnectionBinding,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        let (store, shared_host) = self.session_store()?;
        let now = shared_host.now();
        let state_id = runtime_primitives::IdSource::new_id(shared_host.as_ref()).to_string();
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    crate::machine_notice_store::remember_peer(scope, binding.peer, state_id)
                        .await?;
                    let effects = fleet_sync_session::connection_opened(scope, &binding, now)
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
        binding: fleet_sync_session::ConnectionBinding,
        frame: Vec<u8>,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        let ObjectWork::Session { installation, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            });
        };
        let installation = installation.clone();
        let (store, shared_host) = self.session_store()?;
        let forward_frame = fleet_sync_protocol::FrameId::new(
            runtime_primitives::IdSource::new_id(shared_host.as_ref()),
        )
        .map_err(StoreError::backend)?;
        let now = shared_host.now();
        let state_id = runtime_primitives::IdSource::new_id(shared_host.as_ref()).to_string();
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    crate::machine_notice_store::remember_peer(scope, binding.peer, state_id)
                        .await?;
                    let effects = fleet_sync_session::frame_received(scope, &binding, &frame, now)
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
        session: fleet_sync_session::SessionKey,
        after: fleet_sync_protocol::NoticePosition,
    ) -> Result<
        Vec<fleet_sync_session::StoredNotice<board_sync_protocol::BoardConsumerProtocol>>,
        StoreError,
    > {
        let (store, _) = self.session_store()?;
        store.transaction(move |scope|Box::pin(async move {
            fleet_sync_session::notices_after::<_,board_sync_protocol::BoardConsumerProtocol>(scope,&session,after)
                .await.map_err(StoreError::backend)
        })).await
    }

    #[cfg(feature = "conformance")]
    pub async fn conformance_frame(
        &self,
        binding: fleet_sync_session::ConnectionBinding,
        frame: Vec<u8>,
    ) -> Result<(), StoreError> {
        self.on_frame(binding, frame).await
    }

    pub async fn on_socket_closed(
        &self,
        binding: fleet_sync_session::ConnectionBinding,
    ) -> Result<(), StoreError> {
        use runtime_primitives::Clock;
        let (store, shared_host) = self.session_store()?;
        let now = shared_host.now();
        let result = store
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
                    scope.stage_transport(effects, Some(binding.connection))
                })
            })
            .await;
        if let ObjectWork::Session { replies, .. } = &self.work {
            replies.fail_connection(binding.connection);
        }
        result
    }

    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Session {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            });
        };
        let bootstrap = crate::ObjectBootstrap::new(
            host.clone(),
            runtime_primitives::Clock::now(shared_host.as_ref()),
        );
        let session_migrations = bootstrap.initialize(crate::EngineSchema::Session).await?;
        // The delivery corpus owns the status stream as well as mailbox state.
        let delivery_migrations = bootstrap.initialize(crate::EngineSchema::Mailbox).await?;
        session_migrations
            .checked_add(delivery_migrations)
            .ok_or_else(|| StoreError::Aborted {
                reason: "applied migration count overflow".into(),
            })
    }

    #[cfg(feature = "conformance")]
    pub async fn conformance_closed(
        &self,
        binding: fleet_sync_session::ConnectionBinding,
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
