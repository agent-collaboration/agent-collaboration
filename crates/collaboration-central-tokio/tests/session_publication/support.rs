use collaboration_central_core::{
    CoreHost, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, RegistrationEdits,
    RegistrationSnapshot, SessionKind, SocketFailure,
};
use collaboration_central_tokio::{SocketCommand, TokioStorageHost};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, NotSentReason, ReborrowScope, StoreError,
    storage::TransactionScope,
};
use fleet_sync_protocol::{
    ClientFrame, CloseReason, ConnectionId, NoConsumer, PeerId, ReceiverId, SchemaSupport,
    SessionId,
};
use fleet_sync_session::{Bytes, ConnectionBinding, ConnectionRegistration, VerifiedPeer};
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::{query::query, row::Row};
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering},
    },
};
use tokio::sync::{Semaphore, mpsc};

pub type TestError = Box<dyn std::error::Error + Send + Sync>;
pub type Runtime = ObjectRuntime<SessionKind, TestHost, Transport>;
type NativeObject = TokioObject<CoreHost<Transport>>;

#[derive(Clone, Default)]
pub struct Transport {
    pub calls: Arc<Mutex<Vec<serde_json::Value>>>,
    pub wakes: Arc<Mutex<Vec<SyncTime>>>,
    pub gather: Arc<PublicationProbe>,
    pub schema: Arc<PublicationProbe>,
    pub clock_offset_millis: Arc<AtomicI64>,
}
impl Clock for Transport {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(
            1_792_281_600_000_i64.saturating_add(self.clock_offset_millis.load(Ordering::SeqCst)),
        )
    }
}
impl IdSource for Transport {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for Transport {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.wakes
            .lock()
            .map_err(|_| WakeFailure::Unavailable {
                message: "wake mutex poisoned".into(),
            })?
            .push(at);
        Ok(())
    }
}
impl PlatformIo for Transport {
    async fn object_call(
        &self,
        _to: ObjectAddress,
        bytes: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        let call: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| CallFailure::AnswerLost)?;
        self.calls
            .lock()
            .map_err(|_| CallFailure::AnswerLost)?
            .push(call.clone());
        match call.get("kind").and_then(serde_json::Value::as_str) {
            Some("fleet-sync.report-schema-support") => {
                if self.schema.hold.load(Ordering::SeqCst) {
                    self.schema.entered.add_permits(1);
                    self.schema
                        .release
                        .acquire()
                        .await
                        .map_err(|_| CallFailure::AnswerLost)?
                        .forget();
                }
                Ok(b"null".to_vec())
            }
            Some("central.welcome-leases") => {
                if self.gather.hold.load(Ordering::SeqCst) {
                    self.gather.entered.add_permits(1);
                    self.gather
                        .release
                        .acquire()
                        .await
                        .map_err(|_| CallFailure::AnswerLost)?
                        .forget();
                }
                Ok(b"[]".to_vec())
            }
            _ => Err(CallFailure::NotSent {
                reason: NotSentReason::Overloaded,
            }),
        }
    }
}

pub struct PublicationProbe {
    pub fail: Mutex<Option<ConnectionId>>,
    pub hold: AtomicBool,
    pub entered: Semaphore,
    pub release: Semaphore,
}
impl Default for PublicationProbe {
    fn default() -> Self {
        Self {
            fail: Mutex::default(),
            hold: AtomicBool::new(false),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }
}

pub struct TestStore {
    pub object: Arc<NativeObject>,
    pub rollback: Arc<AtomicU8>,
    pub injected: Arc<AtomicBool>,
    pub after_rollback: Arc<Mutex<Option<String>>>,
}
impl ReborrowScope for TestStore {
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        NativeObject::reborrow(scope)
    }
}
impl AtomicStore for TestStore {
    type Scope<'transaction> = <NativeObject as AtomicStore>::Scope<'transaction>;
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
        let rollback = self.rollback.clone();
        let injected = self.injected.clone();
        let result = self.object.transaction(move |scope| Box::pin(async move {
            let output = run(scope).await?;
            let fault = rollback.load(Ordering::SeqCst);
            if fault != 0 {
                // This is the real journal row written after the finalization callback.
                let row = query::<sqlx_turso::Turso>("SELECT CAST(COUNT(*) AS TEXT) AS count FROM _fleet_sync_steps WHERE step_name = 'attach'").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
                let count: String = row.try_get("count").map_err(StoreError::backend)?;
                if count != "0" && rollback.swap(0, Ordering::SeqCst) != 0 {
                    injected.store(true, Ordering::SeqCst);
                    if fault == 1 { return Err(StoreError::Aborted { reason: "injected callback error after staged Welcome and attach receipt".into() }); }
                    query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_attach_sequences (peer, sequence) SELECT peer, sequence FROM _fleet_sync_attach_sequences LIMIT 1").execute(scope.executor()).await.map_err(StoreError::backend)?;
                }
            }
            Ok(output)
        })).await;
        if result.is_err() && self.injected.load(Ordering::SeqCst) {
            let count = self.object.transaction(|scope| Box::pin(async move {
                let row = query::<sqlx_turso::Turso>("SELECT CAST(COUNT(*) AS TEXT) AS count FROM _fleet_sync_steps WHERE step_name = 'attach'").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
                row.try_get::<String, _>("count").map_err(StoreError::backend)
            })).await?;
            *self.after_rollback.lock().map_err(StoreError::backend)? = Some(count);
        }
        result
    }
}

pub struct TestHost {
    pub native: Arc<TokioStorageHost<CoreHost<Transport>>>,
    pub store: Arc<TestStore>,
    pub probe: Arc<PublicationProbe>,
}
impl ObjectHost for TestHost {
    type Database = sqlx_turso::Turso;
    type Store = TestStore;
    type Wake = CoreHost<Transport>;
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
    async fn admission_binding(
        &self,
        connection: fleet_sync_protocol::ConnectionId,
    ) -> Result<fleet_sync_session::ConnectionBinding, collaboration_central_core::SocketFailure>
    {
        self.native.admission_binding(connection).await
    }
    async fn admission_origin(
        &self,
        connection: ConnectionId,
    ) -> Result<collaboration_central_core::AdmissionOrigin, SocketFailure> {
        self.native.admission_origin(connection).await
    }
    async fn registrations(&self) -> Result<RegistrationSnapshot, SocketFailure> {
        self.native.registrations().await
    }
    async fn publish_registrations(
        &self,
        mut edits: RegistrationEdits,
    ) -> Vec<(ConnectionId, SocketFailure)> {
        if edits
            .values()
            .any(|edit| matches!(edit, ConnectionRegistration::Attached { .. }))
        {
            if self.probe.hold.load(Ordering::SeqCst) {
                self.probe.entered.add_permits(1);
                match self.probe.release.acquire().await {
                    Ok(permit) => permit.forget(),
                    Err(_) => {
                        return edits
                            .keys()
                            .map(|id| (*id, SocketFailure::Closed))
                            .collect();
                    }
                }
            }
            let failed = self
                .probe
                .fail
                .lock()
                .ok()
                .and_then(|connection| *connection);
            if let Some(connection) = failed
                && edits.remove(&connection).is_some()
            {
                let mut failures = self.native.publish_registrations(edits).await;
                failures.push((connection, SocketFailure::Closed));
                return failures;
            }
        }
        self.native.publish_registrations(edits).await
    }
}

pub struct Fixture {
    pub objects: ObjectStore<CoreHost<Transport>>,
    pub host: Arc<TestHost>,
    pub runtime: Arc<Runtime>,
    pub io: Transport,
}
impl Fixture {
    pub async fn open(path: &std::path::Path) -> Result<Self, TestError> {
        Self::open_with(path, true, Self::default_installation()?).await
    }
    pub async fn reopen(path: &std::path::Path) -> Result<Self, TestError> {
        Self::open_with(path, false, Self::default_installation()?).await
    }
    #[allow(
        dead_code,
        reason = "Used by multi-installation holder and forward cases sharing this fixture."
    )]
    pub async fn open_for_installation(
        path: &std::path::Path,
        installation: message_board::ServiceId,
    ) -> Result<Self, TestError> {
        Self::open_with(path, true, installation).await
    }
    fn default_installation() -> Result<message_board::ServiceId, TestError> {
        Ok(message_board::ServiceId::try_from(
            "019f0000-0000-7000-8000-000000000001".to_owned(),
        )?)
    }
    async fn open_with(
        path: &std::path::Path,
        initialize: bool,
        installation: message_board::ServiceId,
    ) -> Result<Self, TestError> {
        let io = Transport::default();
        let receiver = ReceiverId::new(durable_steps::Uuid::now_v7())?;
        let objects = ObjectStore::open(path).await?;
        let shared = Arc::new(CoreHost::conformance_registry(io.clone(), receiver));
        let object = objects
            .object(ObjectKey::new("session")?, Default::default(), shared)
            .await?;
        let native = Arc::new(TokioStorageHost::new(object.clone()));
        let host = Arc::new(TestHost {
            native,
            store: Arc::new(TestStore {
                object,
                rollback: Arc::new(AtomicU8::new(0)),
                injected: Arc::new(AtomicBool::new(false)),
                after_rollback: Arc::default(),
            }),
            probe: Arc::default(),
        });
        // This route address is a test fixture only; no provenance is derived from PeerId.
        let runtime = Arc::new(Runtime::session(
            host.clone(),
            io.clone(),
            receiver,
            installation,
            std::time::Duration::from_secs(2),
        )?);
        if initialize {
            runtime.initialize().await?;
        }
        Ok(Self {
            objects,
            host,
            runtime,
            io,
        })
    }
    pub async fn socket(
        &self,
    ) -> Result<(ConnectionBinding, SessionId, mpsc::Receiver<SocketCommand>), TestError> {
        self.socket_for(PeerId::new(durable_steps::Uuid::now_v7())?)
            .await
    }
    pub async fn socket_for(
        &self,
        peer: PeerId,
    ) -> Result<(ConnectionBinding, SessionId, mpsc::Receiver<SocketCommand>), TestError> {
        let connection = ConnectionId::new(durable_steps::Uuid::now_v7())?;
        let session = SessionId::new(durable_steps::Uuid::now_v7())?;
        let binding = ConnectionBinding::new(
            connection,
            VerifiedPeer {
                peer,
                expires_at: SyncTime::from_unix_millis(1_792_285_200_000),
            },
        );
        let origin = collaboration_central_core::AdmissionOrigin {
            installation: message_board::ServiceId::try_from(
                "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            )?,
            machine: message_board::MachineLabel::try_from("native fixture".to_owned())?,
        };
        let socket = self.host.native.attach_socket(binding, origin).await;
        Ok((binding, session, socket))
    }
    pub async fn hello(
        &self,
        binding: ConnectionBinding,
        session_id: SessionId,
    ) -> Result<(), TestError> {
        let hello = serde_json::to_vec(&ClientFrame::<NoConsumer>::Hello {
            session_id,
            schema_support: SchemaSupport { max_version: 7 },
        })?;
        self.runtime.conformance_frame(binding, hello).await?;
        Ok(())
    }
    pub async fn attached_receipts(&self) -> Result<String, StoreError> {
        self.host.store.object.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT CAST(COUNT(*) AS TEXT) AS count FROM _fleet_sync_steps WHERE step_name = 'attach'").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            row.try_get("count").map_err(StoreError::backend)
        })).await
    }
}
