//! The SDK commits and creates notice frames; this fixture owns only its native scope.
use board_sync_protocol::{BoardConsumerProtocol, BoardNotice, PositionedBoardNotice};
use durable_steps::{
    AtomicStore, CallFailure, Host, StoreError, storage::TransactionScope, turso_store::TursoScope,
};
use fleet_sync_protocol::{ClientFrame, ConnectionId, Notice, NoticePosition, PeerId, SyncTime};
use fleet_sync_session::{
    ConnectionBinding, ConnectionRegistration, PublishSchemaSupportInput, SessionEffect,
    SessionKey, SessionScope, VerifiedPeer, complete_schema_publication, connection_opened,
    frame_received, publish_notice_to_peer,
};
use fleet_sync_tokio_runner::{
    CallEnvelope, CallRouter, ObjectKey, ObjectOptions, ObjectStore, TokioHost, TokioObject,
};
use runtime_primitives::{Clock, SystemClock};
use serde_json::Value;
use std::{collections::BTreeMap, io, path::Path, sync::Arc, time::Duration};
use tokio::sync::Mutex;

pub(super) type TestError = Box<dyn std::error::Error + Send + Sync>;
struct NoNoticeCalls;
impl CallRouter for NoNoticeCalls {
    async fn dispatch(&self, _: CallEnvelope) -> Result<Value, CallFailure> {
        Err(CallFailure::AnswerLost)
    }
}
type NoticeHost = TokioHost<NoNoticeCalls, SystemClock>;
type NoticeStore = TokioObject<NoticeHost>;

struct NoticeScope<'scope, 'transaction, H: Host> {
    inner: &'scope mut TursoScope<'transaction, H>,
    registrations: BTreeMap<ConnectionId, ConnectionRegistration>,
    publications: Vec<PublishSchemaSupportInput>,
}
impl<H: Host> TransactionScope for NoticeScope<'_, '_, H> {
    type Database = <TursoScope<'static, H> as TransactionScope>::Database;
    type Executor<'query>
        = <TursoScope<'static, H> as TransactionScope>::Executor<'query>
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.inner.executor()
    }
}
impl<H: Host> SessionScope for NoticeScope<'_, '_, H> {
    fn registration(&self, connection: &ConnectionId) -> Option<ConnectionRegistration> {
        self.registrations.get(connection).copied()
    }
    fn set_registration(
        &mut self,
        connection: &ConnectionId,
        registration: ConnectionRegistration,
    ) {
        self.registrations.insert(*connection, registration);
    }
    async fn publish_schema_support(
        &mut self,
        input: PublishSchemaSupportInput,
        _: SyncTime,
    ) -> Result<(), fleet_sync_session::StoreError> {
        self.publications.push(input);
        Ok(())
    }
    fn welcome_after_commit(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: fleet_sync_protocol::SessionId,
    ) {
        // The generic FleetService supplies the actual Welcome. This separate
        // notice ledger supplies no fabricated registry/copy status or frame.
    }
}

pub(super) struct NoticeLedger {
    owners: ObjectStore<NoticeHost>,
    store: Arc<NoticeStore>,
    peer: PeerId,
    registrations: Mutex<BTreeMap<ConnectionId, ConnectionRegistration>>,
    key: Mutex<Option<SessionKey>>,
}
impl NoticeLedger {
    pub async fn open(directory: &Path, peer: PeerId) -> Result<Arc<Self>, TestError> {
        let owners = ObjectStore::open(directory.join("notice-source")).await?;
        let store = owners
            .object(
                ObjectKey::new("session")?,
                ObjectOptions::default(),
                Arc::new(TokioHost::new(Arc::new(NoNoticeCalls), SystemClock)),
            )
            .await?;
        store
            .transaction(|scope| {
                Box::pin(async move {
                    let mut migrator = sqlx::migrate::Migrator::with_migrations(
                        fleet_sync_session::SESSION_MIGRATOR
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>(),
                    );
                    migrator.dangerous_set_table_name("_fleet_sync_session_history");
                    migrator
                        .run_direct(None, scope.executor(), false)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await?;
        Ok(Arc::new(Self {
            owners,
            store,
            peer,
            registrations: Mutex::new(BTreeMap::new()),
            key: Mutex::new(None),
        }))
    }

    /// Input is the Hello actually encoded by SessionClient, never a fixture-built frame.
    pub async fn attach(&self, hello: &[u8]) -> Result<Vec<fleet_sync_session::Bytes>, TestError> {
        let ClientFrame::<BoardConsumerProtocol>::Hello { session_id, .. } =
            serde_json::from_slice(hello)?
        else {
            return Err(io::Error::other("notice source expected the actual client Hello").into());
        };
        let now = SystemClock.now();
        let binding = ConnectionBinding::new(
            ConnectionId::new(durable_steps::Uuid::now_v7())?,
            VerifiedPeer {
                peer: self.peer,
                expires_at: now.after(Duration::from_secs(3600)),
            },
        );
        let hello = hello.to_vec();
        let mut registrations = self.registrations.lock().await;
        let staged = registrations.clone();
        let (committed, effects) = self
            .store
            .transaction(move |inner| {
                Box::pin(async move {
                    let mut scope = NoticeScope {
                        inner,
                        registrations: staged,
                        publications: Vec::new(),
                    };
                    connection_opened::<_, BoardConsumerProtocol>(&mut scope, &binding, now)
                        .await
                        .map_err(StoreError::backend)?;
                    let effects = frame_received::<_, BoardConsumerProtocol>(
                        &mut scope, &binding, &hello, now,
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    // Known schema support is the fixture's registry stand-in. The SDK's
                    // completion still owns the committed session/registration transitions.
                    for publication in std::mem::take(&mut scope.publications) {
                        complete_schema_publication(&mut scope, &publication, now)
                            .await
                            .map_err(StoreError::backend)?;
                    }
                    Ok((scope.registrations, effects))
                })
            })
            .await?;
        *registrations = committed;
        *self.key.lock().await = Some(SessionKey {
            peer: self.peer,
            session_id,
        });
        Self::notice_frames(effects)
    }

    pub async fn publish(
        &self,
        position: u64,
        stream: fleet_sync_protocol::DeliveryStream,
    ) -> Result<Vec<fleet_sync_session::Bytes>, TestError> {
        if self.key.lock().await.is_none() {
            return Err(io::Error::other("notice source has not seen Hello").into());
        }
        let mut registrations = self.registrations.lock().await;
        let staged = registrations.clone();
        let peer = self.peer;
        let (committed, effects) = self
            .store
            .transaction(move |inner| {
                Box::pin(async move {
                    let mut scope = NoticeScope {
                        inner,
                        registrations: staged,
                        publications: Vec::new(),
                    };
                    let effects = publish_notice_to_peer::<_, BoardConsumerProtocol>(
                        &mut scope,
                        &peer,
                        Notice::Consumer(PositionedBoardNotice {
                            position: NoticePosition::new(position),
                            kind: BoardNotice::MailWaiting { stream },
                        }),
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    Ok((scope.registrations, effects))
                })
            })
            .await?;
        *registrations = committed;
        Self::notice_frames(effects)
    }
    fn notice_frames(
        effects: Vec<SessionEffect<BoardConsumerProtocol>>,
    ) -> Result<Vec<fleet_sync_session::Bytes>, TestError> {
        effects
            .into_iter()
            .filter_map(|effect| match effect {
                SessionEffect::Send {
                    frame: frame @ fleet_sync_protocol::ServerFrame::Notice { .. },
                    ..
                } => Some(
                    serde_json::to_vec(&frame)
                        .map(Into::into)
                        .map_err(Into::into),
                ),
                _ => None,
            })
            .collect()
    }
    pub async fn close(self) -> Result<(), TestError> {
        let Self { owners, store, .. } = self;
        drop(store);
        owners.close().await?;
        Ok(())
    }
}
